# Search and Eggsearch Integration Roadmap

Status: closed

Long-term references:

- `plans/000-long-term-specification.md#42-explicit-ownership`
- `plans/000-long-term-specification.md#46-progressive-disclosure`
- `plans/000-long-term-specification.md#7-current-foundation-and-required-evolution`
- `plans/003-planning-process.md#7-corrective-passes`

Related ADRs:

- None. The ownership decision is already present in current CodeGG architecture: CodeGG owns the stable agent-facing tool surface and policy/framing boundary; eggsearch owns external search, fetch, provider routing, and evidence discovery. Create an ADR only if implementation evidence requires changing that ownership boundary.

Historical integration context:

- `5403bc66dbc99978dac0a94f18976f6020a050f3` — expanded native wrappers for eggsearch MCP tools.
- `0cc2fab304d2e2489268de0958f8dc3b8f3e81d7` — hardened eggsearch integration and trust/output handling.
- `e185e716d7879db6eaf79633703c2ac6bbd2a15b` — mock-backed eggsearch integration test expansion.
- `72b3b289a2fb2db44db790030d8b3b364f498d39` — last explicit compatibility validation, against eggsearch 0.3.4.

Corrective trigger:

A 2026-08-15 audit of CodeGG `main` at `40dbd1981abf1a8d96d7ab9f5ebefb4b763053f2` against eggsearch 0.3.6 (`4ccb374af00348bba75761f6bbd1e192d385a2b9`) found that CodeGG still defaults to eggsearch correctly, but several wrapper request schemas have drifted and two other external-search implementations bypass eggsearch entirely.

## 1. Purpose and ownership boundary

This subsystem owns the integration boundary between CodeGG and eggsearch.

CodeGG owns:

- stable agent-facing tool names and ergonomic CodeGG argument shapes;
- permission classification and exposure policy;
- whether raw MCP tools are hidden from the model;
- trust classification and model-facing framing;
- bounded output/context handling;
- tool provenance and structured result plumbing;
- orchestration and synthesis performed after evidence is retrieved;
- explicit compatibility/fallback policy.

Eggsearch owns:

- external web search and explicit URL fetch;
- repository discovery, repository fetch, and repository map retrieval;
- security/advisory discovery;
- research-oriented external evidence discovery;
- external provider selection, routing, health/capability reporting, and keyless fallback;
- provider-specific HTTP clients and credentials for search providers;
- search-result identities, structured warnings, trust markers, routing decisions, next actions, retrieval metadata, and evidence-bundle semantics.

CodeGG MUST NOT grow another first-class external search/provider stack in parallel with eggsearch. Local filesystem search, grep, LSP, Git, and local workspace inspection remain CodeGG responsibilities and are not part of this boundary.

The legacy implementation under `src/search/` MAY remain as an explicit compatibility fallback, but it is not a second primary backend and must not receive new provider features.

## 2. Work classification

### Invariants

- `SearchConfig::backend()` continues to default to eggsearch.
- `fallback_to_builtin` remains false by default.
- Raw `mcp__eggsearch__*` tools remain hidden from the model by default; CodeGG wrappers are the normal agent-facing surface.
- External search/fetch evidence is never treated as instruction-trusted content.
- New external search providers are implemented in eggsearch, not CodeGG.
- A CodeGG compatibility wrapper must never silently discard a user/model constraint because the upstream field changed; it must translate it, reject it clearly, or remove it from the exposed schema.
- Baseline eggsearch use must not require CodeGG to prompt for provider credentials; eggsearch's keyless behavior remains available.
- Local search and local workspace tools remain available independently of external search.

### Capabilities

- All CodeGG eggsearch wrappers successfully invoke the current supported eggsearch contract.
- `codesearch` no longer bypasses eggsearch through a direct Exa HTTP client.
- CodeGG deep-research external discovery no longer bypasses eggsearch through direct Tavily, Brave, SerpAPI, or Kagi clients.
- Structured eggsearch response metadata survives the CodeGG wrapper boundary for internal consumers while the model continues receiving bounded, trust-framed output.
- `codegg doctor search` reports enough contract/capability information to diagnose an incompatible eggsearch installation.

### Infrastructure

- Canonical request translation helpers for repository locators, line ranges, batch items, security identifiers, and research options.
- Parsed structured response values carried through `StructuredToolResult::value` rather than discarded into an opaque string-only path.
- Narrow compatibility fixtures/tests that validate actual request contracts rather than merely checking MCP tool names.

### Polish

- Remove stale documentation, provider lists, and historical repository references that imply CodeGG owns provider expansion.
- Keep user-facing tool descriptions aligned with the supported eggsearch feature subset.

## 3. Non-goals

- Vendoring eggsearch into the CodeGG workspace.
- Reimplementing eggsearch provider routing in CodeGG.
- Mirroring every eggsearch request field in every CodeGG wrapper.
- Replacing CodeGG's local `grep`, `glob`, LSP, Git, or workspace inspection tools with eggsearch.
- Deleting the legacy built-in fallback before compatibility value and removal criteria are established.
- Rewriting the entire CodeGG research synthesis pipeline. The correction concerns external evidence collection, not claim synthesis or report rendering unless those layers depend directly on duplicated search clients.
- Adding scheduled compatibility jobs, a version matrix, dependency bot, new CI lane, or network-dependent CI gate.
- Pinning CodeGG to one exact eggsearch patch forever. The audited baseline is 0.3.6; later compatible versions should work through capability/schema discipline.
- Broad MCP redesign unrelated to preserving eggsearch structured responses.

## 4. Current state

At the audited CodeGG baseline:

- `SearchConfig::backend()` defaults to `Eggsearch`.
- `expose_raw_mcp_tools` defaults false.
- `fallback_to_builtin` defaults false.
- `websearch` and `webfetch` dispatch through `search_backend` and eggsearch by default.
- expanded wrappers exist for `repo_search`, `repo_fetch`, `repo_map`, `security_search`, `research_search`, `batch_fetch`, and `evidence_bundle`.
- `src/search/*` remains an explicit built-in/legacy provider implementation.
- `codesearch` is always registered and directly calls Exa Code API using `EXA_API_KEY` / `EXA_CODE_API_KEY`, bypassing eggsearch.
- `src/research/sources/search_provider.rs` contains direct Tavily, Brave, SerpAPI, and Kagi HTTP clients, creating a second latent external-search provider stack.

Compatibility review against eggsearch 0.3.6 found:

- `web_search`: current core fields remain accepted; CodeGG does not expose several useful current hints, but there is no primary breakage.
- `web_fetch`: current core fields remain accepted; newer PDF/browser/cache capabilities are intentionally not required for parity in the first correction.
- `repo_search`: basic request remains usable but CodeGG's combined repo locator and stale `include_snippets` shape do not reflect the current structured contract.
- `repo_fetch`: broken request shape. Current eggsearch requires separate `owner`, `repo`, and `path`; CodeGG sends a combined repo locator, omits `owner`, and uses `start_line` / `end_line` rather than `line_start` / `line_end`.
- `repo_map`: broken request shape. Current eggsearch requires `owner` and `repo`, and uses `max_depth`; CodeGG sends a combined `repo`, `path`, and `depth`.
- `security_search`: generic query works, but CodeGG forwards `cve` while current eggsearch expects `cve_id`; other structured identifiers and applicability fields are unavailable.
- `research_search`: generic query works, but CodeGG forwards stale `domains`; current eggsearch uses fields such as `research_domain`, `desired_source_types`, `providers`, `workflow`, and `depth`.
- `batch_fetch`: broken for the current contract. Eggsearch requires non-empty tagged `items`; CodeGG may send a top-level `urls` array and its repo item shape lacks the current required structured locator fields.
- `evidence_bundle`: CodeGG exposes historical source descriptors rather than the current source-card/fetch evidence inputs and therefore cannot reliably preserve source linkage and trust metadata.

The existing fake MCP integration tests register permissive empty schemas and accept arbitrary JSON. They prove that CodeGG reaches a named MCP tool, but they do not validate that current eggsearch can deserialize the request. This is why schema drift survived earlier verification.

Current eggsearch also publishes a stable harness-facing response contract including deterministic `stable_id` values, `structured_warnings`, `trust_markers`, `next_actions`, `routing_decision`, retrieval-state metadata, and capability discovery. CodeGG currently treats most of that response as opaque text and may byte-cap the serialized result before internal consumers can use those fields.

## 5. Target architecture

```text
Agent / CodeGG orchestration
          |
          v
stable CodeGG tool facade
(websearch, webfetch, repo_*, security_search,
 research_search, batch_fetch, evidence_bundle,
 compatibility aliases where retained)
          |
          +--> CodeGG permission/exposure/trust/provenance policy
          |
          v
one search_backend integration boundary
          |
          v
eggsearch MCP
  |-- generic web search/fetch
  |-- repository/code evidence
  |-- security evidence
  |-- research evidence
  |-- provider routing/capabilities
  `-- structured evidence metadata

Explicit compatibility-only branch:
search.backend = "builtin"
          |
          `--> legacy src/search/*
```

There must be no active CodeGG-owned direct Exa/Tavily/Brave/SerpAPI/Kagi execution path beside this boundary.

For CodeGG's deep-research subsystem, eggsearch is the external evidence collector. CodeGG may continue to own local-source collection, research budgeting/orchestration, claim construction, verification, synthesis, persistence, and report generation.

## 6. Dependency graph

```text
M001 — Current eggsearch contract repair
   |
   | hard
   v
M002 — External search ownership consolidation
   |
   | hard
   v
M003 — Structured contract consumption and compatibility closure
```

Dependency classification:

- M001 -> M002: **hard**. Do not redirect `codesearch` or research-provider traffic into wrappers whose current request contracts are still broken.
- M002 -> M003: **hard**. Structured compatibility closure must measure the final single-owner search architecture, not preserve metadata for paths that are about to be deleted.
- eggsearch 0.3.6 contract documentation/tool schemas: **interface** dependency for M001 and M003.
- availability of a locally runnable current eggsearch binary: **operational** dependency for M003 closure only. M001/M002 implementation must not be blocked by network-dependent CI.

## 7. Milestones

### Milestone 001 — Current eggsearch request-contract repair

Class: capability correctness / compatibility

Objective:

Make every CodeGG eggsearch wrapper emit requests that current eggsearch 0.3.6 accepts, while preserving reasonable compatibility aliases for existing CodeGG argument shapes and eliminating silently ignored fields.

Dependencies:

- none beyond the documented eggsearch 0.3.6 MCP contract.

Deliverable boundary:

- corrected wrapper schemas and translation logic;
- explicit legacy alias handling where semantics are unambiguous;
- strict request-shape regression tests for all supported eggsearch wrappers;
- updated search architecture/tool documentation.

User or operator value:

Repository, security, research, batch-fetch, and evidence workflows stop failing or silently degrading because CodeGG emits stale MCP arguments.

Exit conditions:

- every advertised CodeGG eggsearch wrapper has a documented current upstream mapping;
- `repo_fetch`, `repo_map`, and `batch_fetch` requests satisfy current required fields;
- security/research stale fields are translated or rejected explicitly rather than ignored;
- evidence-bundle input matches current eggsearch semantics;
- mock tests validate required fields and argument names rather than accepting arbitrary JSON;
- focused tests and `scripts/verify.sh quick` are green.

Deferred work:

- deletion/aliasing of competing CodeGG provider clients;
- deeper structured-response consumption;
- network-dependent local compatibility smoke.

### Milestone 002 — External search ownership consolidation

Class: invariant / simplification

Objective:

Make eggsearch the sole normal owner of external search/provider execution in CodeGG.

Dependencies:

- hard: M001 closed.

Deliverable boundary:

- direct Exa `codesearch` execution removed; if the model-facing name is retained, it becomes a thin compatibility alias over eggsearch repository/coding search;
- direct Tavily/Brave/SerpAPI/Kagi research-provider execution removed from CodeGG's external research collection path;
- CodeGG deep research uses eggsearch for network evidence while retaining CodeGG-owned local/synthesis responsibilities;
- `src/search/*` is explicitly compatibility-only and unreachable from default operation except configured fallback semantics;
- docs/config/tool registry reflect one primary external-search owner.

User or operator value:

One provider configuration/routing system, fewer duplicate HTTP clients and credential paths, consistent provenance/trust behavior, and fewer cases where two CodeGG search tools return materially different semantics for the same task.

Exit conditions:

- no normally registered CodeGG tool performs direct external search-provider HTTP calls outside eggsearch;
- no research network collector directly calls Tavily, Brave, SerpAPI, Kagi, or Exa;
- local search capabilities are unchanged;
- explicit `backend = "builtin"` fallback still behaves according to its documented compatibility contract;
- no new CI/static-analysis machinery was introduced solely to enforce ownership.

Deferred work:

- removal of the legacy built-in fallback itself;
- provider features that belong upstream in eggsearch.

### Milestone 003 — Structured contract consumption and compatibility closure

Status: closed — `plans/closure/search-eggsearch-integration/003-status.md`

Implementation: `89dbac7`

Class: infrastructure / compatibility closure

Objective:

Consume the stable machine-readable eggsearch response contract instead of flattening it to opaque text, improve capability diagnostics, and demonstrate compatibility with a real current eggsearch installation using one bounded local smoke path.

Dependencies:

- hard: M002 closed.
- operational for final closure: current eggsearch binary available locally.

Deliverable boundary:

- parsed eggsearch JSON retained in `StructuredToolResult::value` for wrapper calls;
- string output remains bounded and trust-framed for legacy/model consumers;
- truncation never corrupts the structured value used internally;
- relevant deterministic IDs, structured warnings, trust markers, routing decisions, retrieval metadata, and next-action data remain available to internal consumers rather than being destroyed at the adapter boundary;
- doctor/bootstrap surfaces current server/tool/capability compatibility clearly enough to diagnose missing or incompatible features;
- one local real-binary contract smoke covers the wrapper set against eggsearch 0.3.6 or the current audited successor.

User or operator value:

More reliable evidence chaining, better degraded-provider diagnostics, less repeated searching, and an actionable compatibility failure instead of opaque MCP errors.

Exit conditions:

- wrapper structured results carry parsed upstream values;
- model-facing output remains trust-bounded and does not expose raw eggsearch MCP tools by default;
- real local smoke invokes the supported wrapper set through an actual eggsearch MCP process and records the version/tool inventory;
- no permanent network-dependent CI lane or version matrix is added;
- architecture/docs state the supported compatibility contract and upgrade procedure.

Deferred work:

- automatic execution of eggsearch `next_actions` without normal CodeGG tool policy;
- broad evidence-graph or UI features that consume structured metadata;
- automatic dependency updates.

## 8. Cross-cutting requirements

### Storage and migration

No durable database migration is expected.

Research artifacts already persisted by CodeGG must remain readable. Removing direct research-provider clients must not rewrite historical research-run artifacts merely to change provider ownership.

### Protocol and compatibility

CodeGG's native tool names are the compatibility boundary. Existing unambiguous aliases such as a combined `owner/repo` locator may be accepted and translated internally even when eggsearch now uses separate fields.

Do not silently preserve stale arguments whose semantics cannot be represented. For example, a legacy repo-map subdirectory `path` must either have a correct current equivalent or return an actionable validation error directing the caller to the appropriate search/fetch tool.

Unknown additive eggsearch response fields must be preserved/ignored safely rather than causing failures.

### Security and authorization

External content remains data, not instructions.

Eggsearch trust markers complement but do not replace CodeGG's outer tool trust/provenance policy. Local-trusted eggsearch workspace evidence is provenance-trusted only; it is not instruction-trusted.

Removal of direct provider clients must reduce, not multiply, credential handling in CodeGG. Baseline search must not start prompting for API keys that eggsearch does not require.

### Concurrency, cancellation, and recovery

Keep existing per-call timeouts and CodeGG cancellation ownership. Do not introduce background search workers or unbounded fan-out.

Batch fetch remains bounded by eggsearch and CodeGG output/context limits.

MCP process failure must remain an actionable tool error. Configured legacy fallback semantics apply only where explicitly supported; specialized eggsearch-only tools must not silently switch to unrelated provider behavior.

### Observability and audit

Preserve backend provenance (`mcp`, implementation `eggsearch`) and populate upstream version information when it is available without broad MCP redesign.

Doctor output should distinguish:

- process unavailable;
- required tool missing;
- specialized capability unavailable/degraded;
- compatible tool surface with provider-specific degradation.

### Performance and resource use

The correction should reduce duplicate clients and provider stacks rather than add layers.

Do not instantiate extra eggsearch processes per tool call. Continue using the existing shared MCP service/bootstrap lifecycle.

Do not add indexing, caching, or persistent search databases to CodeGG in this workstream.

### Documentation and operations

Update `architecture/search_backend.md`, `architecture/tool.md`, relevant config documentation, and user installation/doctor guidance where stale.

New provider implementation guidance must point to `eggstack/eggsearch`, not historical repository locations.

## 9. Verification strategy

Verification is intentionally narrow and contract-oriented.

M001 uses deterministic unit/integration tests around CodeGG request translation and stricter MCP mocks. It should prove exact required field names and legacy alias translation for all wrappers.

M002 uses focused registry/research tests and source inspection proving the direct external provider clients are no longer on an executable path. Do not create a new permanent source-scanning guard solely for this deletion unless recurrence evidence later justifies one.

M003 adds one explicit local real-binary compatibility smoke. It may be a test helper or documented command sequence, but it must remain opt-in/local if it needs live external access. Tool-schema/deserialization compatibility can be exercised against the local MCP process without requiring broad Internet-dependent assertions.

For each milestone:

1. run the narrowest affected tests first;
2. run formatting/lint appropriate to touched code;
3. run `scripts/verify.sh quick` before closure;
4. use the full workspace/hosted suite only when the implementation materially affects broader runtime behavior or a focused failure requires escalation.

Do not add another CI matrix, scheduled compatibility job, or release gate for this workstream.

## 10. Risks and decision points

### Upstream schema evolution

Risk: CodeGG may drift again if tests only assert its own mock behavior.

Mitigation: M001 strict request fixtures plus M003 real-process compatibility evidence. Treat eggsearch's documented schema-stability rules and capability discovery as the contract rather than copying internal implementation details unnecessarily.

### Compatibility alias ambiguity

Risk: historical CodeGG fields may not have a one-to-one current eggsearch meaning.

Decision rule: translate only when semantics are clear. Otherwise fail with an actionable message and update the exposed tool schema. Do not silently drop the field.

### Deep-research scope expansion

Risk: replacing provider collectors could become a rewrite of the research subsystem.

Decision rule: M002 changes external evidence ownership only. If claim extraction, synthesis, persistence, or research-run storage requires redesign for unrelated reasons, stop and create a separate research plan.

### Generic MCP response plumbing

Risk: retaining structured eggsearch values may tempt a broad MCP protocol refactor.

Decision rule: prefer the smallest additive path that preserves parsed eggsearch JSON and uses existing `StructuredToolResult::value`. If generic MCP behavior must change materially for all servers, stop and split that work into its own plan.

### Legacy fallback removal

Risk: deleting `src/search/*` in the consolidation pass may remove useful offline/emergency compatibility without evidence.

Decision rule: M002 makes legacy fallback explicitly secondary. Final deletion is deferred until usage/removal criteria are known.

## 11. Completion definition

This roadmap is closed only when all three milestones have accepted closure records and all of the following are true:

- eggsearch is the sole normal external search/provider owner in CodeGG;
- all advertised wrappers are compatible with the accepted current eggsearch contract;
- no normally registered direct Exa/Tavily/Brave/SerpAPI/Kagi search path remains;
- legacy in-tree search is explicit compatibility fallback only;
- structured eggsearch values survive the CodeGG integration boundary;
- `codegg doctor search` provides actionable compatibility/capability diagnostics;
- a real local eggsearch compatibility smoke has been recorded;
- verification remains bounded and no new CI/release overengineering was introduced.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 001 — current eggsearch request-contract repair | closed | `plans/implementation/search-eggsearch-integration/001-current-eggsearch-contract-repair.md` | `plans/closure/search-eggsearch-integration/001-status.md` | — |
| 002 — external search ownership consolidation | closed | `plans/implementation/search-eggsearch-integration/002-external-search-ownership-consolidation.md` | `plans/closure/search-eggsearch-integration/002-status.md` | — |
| 003 — structured contract consumption and compatibility closure | ready | `plans/implementation/search-eggsearch-integration/003-structured-contract-and-compatibility-closure.md` | — | current eggsearch binary operational evidence required for final closure |
