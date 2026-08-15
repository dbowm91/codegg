# Search and Eggsearch Integration Milestone 001 — Current Eggsearch Contract Repair

Status: ready for handoff

Repository baseline:

- CodeGG audited baseline: `40dbd1981abf1a8d96d7ab9f5ebefb4b763053f2`
- planning-roadmap addition: `24c4df7ecdf8477cf27d51e0e92acd777d61427d`
- eggsearch audited baseline: 0.3.6, release commit `4ccb374af00348bba75761f6bbd1e192d385a2b9`

Source roadmap:

- `plans/subsystems/search-eggsearch-integration-roadmap.md#milestone-001--current-eggsearch-request-contract-repair`

Long-term requirements:

- `plans/000-long-term-specification.md#42-explicit-ownership`
- `plans/000-long-term-specification.md#46-progressive-disclosure`
- `plans/000-long-term-specification.md#7-current-foundation-and-required-evolution`

Applicable ADRs:

- None. Preserve the existing CodeGG-wrapper / eggsearch-backend ownership boundary.

Historical predecessor evidence:

- `5403bc66dbc99978dac0a94f18976f6020a050f3` — initial expanded eggsearch wrappers.
- `0cc2fab304d2e2489268de0958f8dc3b8f3e81d7` — initial integration hardening.
- `e185e716d7879db6eaf79633703c2ac6bbd2a15b` — mock-backed integration tests.
- `72b3b289a2fb2db44db790030d8b3b364f498d39` — last explicit eggsearch compatibility check, against 0.3.4.

Primary class: capability correctness / compatibility

## 1. Objective

Repair CodeGG's eggsearch wrapper request contract so every model-facing eggsearch-backed tool emits a request accepted by current eggsearch 0.3.6, without creating a second search abstraction or mirroring eggsearch wholesale.

The milestone must correct real breakage in `repo_fetch`, `repo_map`, and `batch_fetch`; remove or translate stale security/research/evidence argument shapes; preserve unambiguous CodeGG compatibility aliases; strengthen tests so future schema drift cannot pass merely because a fake MCP handler accepts arbitrary JSON; and update the integration documentation to describe the actual current contract.

This milestone does not consolidate competing provider clients yet. It repairs the canonical eggsearch path first so later traffic can safely be routed through it.

## 2. Why this milestone is ready

There are no hard CodeGG dependencies.

The interface dependency is stable enough to implement against:

- eggsearch 0.3.6 publishes ten MCP tools and current request schemas;
- eggsearch documents a stable harness-facing response contract and schema-evolution rules;
- CodeGG already has one `search_backend` adapter boundary and native wrapper tools;
- current failures are argument translation/schema defects, not unresolved ownership questions.

M001 is therefore dependency-ready immediately.

## 3. Current implementation evidence

### 3.1 Correct default ownership already exists

`crates/codegg-config/src/schema.rs` currently makes eggsearch the default search backend, keeps raw eggsearch MCP tools hidden by default, and keeps automatic built-in fallback disabled by default.

`src/tool/mod.rs` registers:

- `websearch`;
- `webfetch`;
- eggsearch-backed `repo_search`, `repo_fetch`, `repo_map`, `security_search`, `research_search`, `batch_fetch`, and `evidence_bundle` when the evidence backend resolves to eggsearch.

`src/search_backend/eggsearch.rs` is already the intended native-to-MCP translation layer.

### 3.2 Current request drift

The 2026-08-15 audit found the following current-state mapping.

#### `web_search`

CodeGG sends `query`, `max_results`, and optional `providers` translated from the historical singular `provider` hint. These fields remain accepted by eggsearch 0.3.6.

Current eggsearch additionally supports `intent`, `freshness`, `safe_search`, and per-request timeout. M001 may expose `intent` and `freshness` because they are stable, high-value retrieval hints, but it must not turn CodeGG's wrapper into a byte-for-byte copy of every upstream option.

#### `web_fetch`

CodeGG's current `url`, `max_chars`, `extract_mode = "text"`, and `include_links = false` request is accepted by eggsearch 0.3.6.

M001 need not expose browser, PDF, cache, or profile features solely for parity. If existing CodeGG inputs already represent `extract_mode` or `include_links`, they should be passed through rather than hard-coded; otherwise those enhancements may remain deferred.

#### `repo_search`

CodeGG currently exposes `query`, combined `repo`, `language`, `max_results`, and stale `include_snippets`.

Eggsearch 0.3.6 accepts a richer structured request containing fields such as `host`, `owner`, `repo`, `path`, `file`, `language`, `symbol`, `profile`, package/version fields, `include_local`, `mode`, `workflow`, and result limits.

The historical combined `repo = "owner/name"` CodeGG locator must not be sent upstream as though it were necessarily a current repository-name field. Introduce one canonical locator translation path that can split a clear `owner/repo` value while also accepting explicit `owner` + `repo` fields.

The stale `include_snippets` field must not be silently forwarded when upstream does not define it.

#### `repo_fetch`

CodeGG currently sends:

```json
{
  "repo": "owner/repo",
  "path": "src/lib.rs",
  "start_line": 1,
  "end_line": 40,
  "symbol": "..."
}
```

Eggsearch 0.3.6 requires separate `owner`, `repo`, and `path`, and current range names are `line_start` and `line_end`. It also supports `host`, `ref_name`, `commit_sha`, context ranges, `symbol_kind`, `match_text`, block expansion, and local preference.

A normal CodeGG request can therefore fail at upstream request deserialization today.

#### `repo_map`

CodeGG currently sends a combined `repo`, optional `path`, and `depth`.

Eggsearch 0.3.6 requires separate `owner` and `repo`, and uses `max_depth`. The current upstream `RepoMapArgs` does not provide CodeGG's historical subdirectory `path` semantic.

A normal CodeGG request omits required `owner` and can fail at deserialization. A non-empty historical `path` must not be silently discarded.

#### `security_search`

CodeGG exposes required `query`, `ecosystem`, `package`, historical `cve`, and `max_results`.

Eggsearch 0.3.6 accepts optional `query`, `ecosystem`, `package`, `version`, `cve_id`, `ghsa_id`, `osv_id`, `rustsec_id`, severity, KEV/exploit/defensive/vendor flags, result limits, provider hints, applicability assessment, dependency files, and workflow.

The direct defect is `cve` -> `cve_id`. M001 should also expose the structured identifier fields that materially reduce ambiguity without making every advanced eggsearch option mandatory.

#### `research_search`

CodeGG exposes `query`, historical `domains`, and `max_results`.

Eggsearch 0.3.6 accepts `research_domain`, `desired_source_types`, counterpoint/primary/recent/security flags, result limits, freshness, providers, workflow, depth, compare targets, constraints, and known context.

`domains` is stale and currently loses meaning. The model-facing schema should use current concepts. Legacy `domains` may be accepted internally only where translation is unambiguous; otherwise return a clear validation error rather than silently dropping it.

#### `batch_fetch`

CodeGG currently exposes either:

```json
{"urls": ["https://example.com"]}
```

or repo items containing historical `repo` + `path` fields.

Eggsearch 0.3.6 requires a non-empty `items` array. Each item is a tagged object:

- web item: `type = "web"`, `url`, optional extraction settings;
- repo item: `type = "repo"`, required `owner`, `repo`, `path`, optional host/ref/range fields.

The current top-level `urls` form can fail because required `items` is absent. Historical repo items are also incomplete for the current structured locator.

#### `evidence_bundle`

CodeGG currently models `sources` as historical descriptors containing fields such as `type`, `query`, `url`, `repo`, and `path`.

Eggsearch 0.3.6 expects source-card-like `EvidenceSourceInput` values with source identity/provenance/trust metadata, plus separate fetch inputs and bundle limits.

The current wrapper cannot faithfully chain current eggsearch search/fetch evidence into the bundle tool.

### 3.3 Existing tests do not prove compatibility

`tests/fake_eggsearch_mcp.rs` registers fake tools with permissive empty input schemas and handlers that accept arbitrary JSON. Existing tests verify tool routing and selected fields but do not exercise upstream deserialization requirements.

This allowed invalid `repo_fetch`, `repo_map`, and `batch_fetch` requests to remain green.

## 4. Invariants that must not regress

- Eggsearch remains CodeGG's default external search backend.
- Raw `mcp__eggsearch__*` tools remain hidden from the model unless explicitly configured otherwise.
- `fallback_to_builtin` remains false by default.
- The stable CodeGG tool facade remains the normal model-facing surface.
- Existing CodeGG argument aliases are preserved only when they can be translated without semantic loss.
- No stale field is silently sent or silently ignored after this milestone.
- External result trust remains `external_untrusted` at the CodeGG wrapper boundary unless a later structured response explicitly distinguishes provenance such as eggsearch local workspace evidence; instruction trust is never granted.
- Output caps and per-tool timeouts remain bounded.
- The shared eggsearch MCP service remains process/shared-state owned; do not spawn one process per call.
- No new provider-specific HTTP client is added to CodeGG.
- The built-in fallback path is not expanded.

## 5. Scope

### In scope

- `src/search_backend/eggsearch.rs` request translation;
- agent-facing schemas/descriptions in `src/tool/websearch.rs`, `webfetch.rs`, `repo_search.rs`, `repo_fetch.rs`, `repo_map.rs`, `security_search.rs`, `research_search.rs`, `batch_fetch.rs`, and `evidence_bundle.rs` as needed;
- small reusable request-normalization helpers inside the search integration boundary;
- current eggsearch required/recommended tool inventory and doctor wording if the tool set description is stale;
- focused request mapping tests;
- stricter fake MCP request validation or equivalent contract fixtures;
- search/tool/config documentation affected by corrected request shapes;
- user install/version guidance if it still names the earlier audited eggsearch release.

### Explicitly out of scope

- deleting or aliasing the direct Exa `codesearch` implementation — M002;
- deleting direct research provider clients — M002;
- changing CodeGG research synthesis/claim logic;
- deleting `src/search/*`;
- generic MCP protocol redesign;
- browser automation, PDF rendering, browser-profile management, or every optional eggsearch fetch feature;
- automatically executing eggsearch `next_actions`;
- persistent search indexes or caches;
- new CI lanes, network-required CI, scheduled compatibility tests, or a version matrix;
- release automation.

## 6. Required production changes

### Core/domain

Add one canonical repository-locator normalization helper owned by the search integration layer.

It must support:

1. explicit `owner` + `repo` as the preferred current form;
2. historical combined `repo = "owner/name"` only when the split is unambiguous;
3. optional `host`/`ref_name` forwarding where the CodeGG wrapper exposes them;
4. actionable validation for malformed/ambiguous locators.

Do not replicate this parsing separately in `repo_search`, `repo_fetch`, `repo_map`, and batch item translation.

Add equivalent small helpers for line-range aliases and batch item normalization where this removes repeated translation logic.

### Storage and migrations

No database or artifact migration.

### Protocol and DTOs

#### Web search

Preserve current `query`, result limit, and historical provider hint behavior.

Prefer eggsearch automatic provider routing when no explicit provider is requested.

Add current `intent` and `freshness` hints if they can be exposed without breaking existing callers. Do not hard-code a static provider enum as the primary routing model when eggsearch owns provider discovery.

#### Web fetch

Keep the accepted current request. Do not enlarge this milestone into browser/PDF work.

If the native CodeGG schema exposes options equivalent to current eggsearch fields, forward them instead of overwriting them with fixed values.

#### Repository search

Normalize repository identity into explicit upstream `owner` and `repo` where available.

Remove `include_snippets` from the model-facing current schema unless CodeGG itself consumes that option before forwarding. Never send an unknown upstream field simply because an old wrapper exposed it.

Expose a small high-value current subset, preferably:

- `host`;
- `owner`;
- `repo`;
- `path`;
- `language`;
- `symbol`;
- `profile` (`generic`, `coding`, `security`, `research`);
- `max_results`;
- `include_local`;
- `mode` (`default`, `exact_error`).

Do not mirror every package/workflow field unless a current CodeGG consumer needs it in M001.

#### Repository fetch

Translate to exact current names:

- `owner`;
- `repo`;
- `path`;
- `line_start`;
- `line_end`;
- optional current host/ref/context/symbol fields exposed by the wrapper.

Continue accepting historical `start_line` / `end_line` as internal aliases for one compatibility window if doing so is low-risk, but expose current names to the model.

#### Repository map

Translate to exact current names:

- `owner`;
- `repo`;
- `max_depth`;
- optional current host/ref/result-limit/include fields actually exposed by CodeGG.

Do not silently send/ignore historical `path`. If no exact upstream semantic exists, remove it from the exposed schema and return a clear compatibility error when legacy callers provide a non-empty value, directing them toward `repo_search` or `repo_fetch`.

#### Security search

Translate historical `cve` to current `cve_id` when provided.

Expose current structured identifiers needed for precise lookups:

- `cve_id`;
- `ghsa_id`;
- `osv_id`;
- `rustsec_id`;
- `ecosystem`;
- `package`;
- `version`;
- result limits.

Keep a generic query path.

Do not require optional provider credentials before attempting baseline security search.

#### Research search

Expose current concepts rather than stale `domains`:

- `query`;
- `research_domain`;
- `desired_source_types` where useful;
- `workflow`;
- `depth`;
- `max_results`;
- optional `providers` only as an advanced explicit override.

A legacy `domains` input may map recognized provider IDs to `providers` or one unambiguous domain to `research_domain`; ambiguous multi-purpose values must fail clearly rather than disappear.

#### Batch fetch

Canonicalize every request to current tagged `items` before MCP invocation.

Support a compatibility translation from legacy top-level `urls` by producing web items:

```json
{"type":"web","url":"..."}
```

Normalize historical repo items containing combined `owner/repo` into current tagged repo items with separate `owner`, `repo`, and `path`.

Reject an empty effective item list before invoking MCP.

Preserve current per-item character limits and enforce CodeGG's aggregate output bounds.

#### Evidence bundle

Replace the historical pseudo-source descriptor schema with the current eggsearch evidence input model at the CodeGG boundary.

The wrapper should accept source-card-derived objects with identifiers, URL/title/snippet/provider/trust/metadata fields and separate fetch inputs where supplied. It need not expose every optional field explicitly if the JSON schema can safely permit current additive fields.

Do not invent a new CodeGG evidence-bundle format that requires a second translation layer.

### Runtime and concurrency

Keep the existing shared `McpService` call path, timeout behavior, cancellation ownership, and output limits.

Do not add retries around validation errors. A request-schema failure is deterministic and should surface immediately.

### Frontend or operator surface

Update `codegg doctor search` text only where it currently overstates compatibility based solely on tool names. M001 may label tool inventory as present/absent but must not claim full schema compatibility from discovery alone.

### Security and authorization

Validate URLs before forwarding as today.

Repository locator parsing must not create filesystem access or local path authority in CodeGG; it only normalizes external/workspace identifiers for eggsearch.

Do not log provider credentials or inject them into model-visible errors.

### Documentation and static guards

Update:

- `architecture/search_backend.md`;
- `architecture/tool.md`;
- `architecture/config.md` if model-facing search config semantics changed;
- `README.md` only if installation/version guidance is stale.

Correct stale references to historical eggsearch repository ownership.

No new permanent static guard is required for M001. Regression tests are the appropriate enforcement mechanism.

## 7. Ordered work packages

### Work package A — Establish the exact current contract matrix

Intent:

Turn the audit findings into one explicit CodeGG->eggsearch argument matrix before changing code.

Required changes:

- enumerate all nine wrapper/dispatch paths plus `provider_status` diagnostics;
- record current CodeGG input names, compatibility aliases, current upstream names, required upstream fields, and intentionally unsupported optional fields;
- identify every currently exposed field that has no current upstream semantic.

Acceptance evidence:

- matrix is represented in tests/comments/docs sufficiently to prevent independent wrapper drift;
- no wrapper field remains in an unknown/assumed state.

### Work package B — Centralize repository and range normalization

Intent:

Remove repeated ad-hoc parsing and ensure repo tools agree on locator semantics.

Required changes:

- implement one `owner/repo` compatibility parser;
- prefer explicit owner/repo fields;
- normalize line range aliases;
- produce actionable validation errors.

Acceptance evidence:

- unit tests cover explicit fields, combined locator, malformed locator, line aliases, and non-GitHub host values where supported.

### Work package C — Repair repo/search/security/research adapters

Intent:

Correct the specialized request shapes that are currently broken or silently degraded.

Required changes:

- update `repo_search` current subset and remove stale forwarding;
- repair `repo_fetch` required owner/repo/path and line names;
- repair `repo_map` required owner/repo/max_depth and path handling;
- map security identifiers correctly;
- replace stale research `domains` semantics with current fields/explicit compatibility behavior.

Acceptance evidence:

- request-capture tests assert exact upstream JSON for representative current and legacy inputs;
- no test expects the stale broken forms.

### Work package D — Repair batch fetch and evidence bundle

Intent:

Restore current multi-fetch and evidence chaining semantics.

Required changes:

- normalize legacy URL arrays to tagged web items;
- normalize repo items to tagged structured locators;
- enforce non-empty items;
- update evidence bundle inputs to current source/fetch model;
- preserve bundle limits without fabricating source identity.

Acceptance evidence:

- web-only, repo-only, mixed batch, empty batch, malformed repo locator, and current evidence-bundle request tests pass.

### Work package E — Make fake MCP tests reject stale requests

Intent:

Prevent recurrence of the exact verification gap that allowed schema drift.

Required changes:

Replace permissive "any object succeeds" behavior for specialized fake eggsearch calls with one of:

- strict per-tool validation helpers matching the supported 0.3.6 subset; or
- captured current `tools/list` schemas/fixtures plus local JSON-schema validation; or
- another deterministic test mechanism that fails when required fields/names are stale.

Do not add eggsearch as a production dependency merely for tests unless that is demonstrably the smallest solution.

Acceptance evidence:

- reverting any of the repaired required fields to the old name/shape causes a focused test failure;
- tests still run offline and deterministically.

### Work package F — Documentation and focused verification

Intent:

Close the contract correction without broadening verification infrastructure.

Required changes:

- update architecture/docs;
- remove stale current-schema claims;
- run focused tests and quick verification.

Acceptance evidence:

- docs and tests describe the same supported subset;
- no new CI lane/version matrix exists.

## 8. Failure, cancellation, restart, and contention semantics

Validation failures must be local, deterministic, and immediate. Do not invoke eggsearch when CodeGG can prove required fields are missing or ambiguous.

MCP transport failure remains a transport/tool error and must retain the existing actionable eggsearch-unavailable behavior.

Timeout behavior remains bounded by existing configured per-domain/default timeouts.

A cancelled parent/tool call must not be converted into a request-schema fallback.

No new persistent state is introduced, so restart semantics are unchanged.

Concurrent callers continue sharing the initialized MCP service. Request normalization must be pure/per-call and must not mutate global search configuration.

## 9. Compatibility and migration

No durable migration.

Compatibility policy:

- prefer current model-facing field names;
- accept historical aliases only where there is a one-to-one semantic mapping;
- combined `repo = "owner/name"` may remain as a compatibility input while explicit owner/repo becomes preferred;
- `start_line`/`end_line` may remain aliases for `line_start`/`line_end`;
- historical `urls` may remain an input alias for batch web items;
- historical `cve` may remain an alias for `cve_id`;
- ambiguous `repo_map.path` and `research_search.domains` values must not be silently ignored.

Document any compatibility alias added and its removal condition. Do not promise indefinite support for undocumented internal JSON shapes.

## 10. Required tests

### Focused unit tests

- repository locator normalization;
- malformed/ambiguous repo locator rejection;
- line-range alias normalization;
- security identifier alias mapping;
- research current-field mapping and ambiguous legacy rejection;
- batch item normalization;
- evidence bundle current input forwarding;
- provider-hint translation remains safe/automatic for unknown hints.

### Integration tests

Update/extend:

- `tests/search_backend_arg_mapping.rs`;
- `tests/search_backend_eggsearch.rs`;
- `tests/fake_eggsearch_mcp.rs`.

Each eggsearch wrapper must have at least one test asserting the upstream tool name and exact required request fields.

Add negative fake-server validation proving stale `repo_fetch`, `repo_map`, and `batch_fetch` forms fail.

### Restart and recovery tests

No new restart test required. Existing bootstrap coverage is sufficient unless implementation changes search process lifecycle.

### Contention and cancellation tests

No new contention suite required. Preserve existing serialized global-state test discipline for `search_backend::state`.

### Security and negative tests

- non-HTTP(S) batch/web URLs rejected before fetch;
- empty query where CodeGG requires one rejected;
- malformed repository locator rejected;
- ambiguous deprecated arguments rejected rather than ignored;
- no credential value appears in errors.

### Migration and compatibility tests

- combined repo locator -> explicit owner/repo;
- `start_line` -> `line_start`;
- `end_line` -> `line_end`;
- legacy `cve` -> `cve_id`;
- legacy `urls` -> tagged web `items`;
- explicitly document/test the behavior of legacy `domains` and repo-map `path`.

## 11. Required verification commands

Use the narrowest exact test targets available after implementation. Expected minimum:

```bash
cargo fmt --all -- --check
git diff --check
cargo test --test search_backend_arg_mapping -- --test-threads=1
cargo test --test search_backend_eggsearch -- --test-threads=1
cargo test --test fake_eggsearch_mcp -- --test-threads=1
scripts/verify.sh quick
```

If test filenames are consolidated during implementation, record the replacement commands in the closure record rather than retaining stale commands.

Do not add or require a live-network test for M001 closure.

## 12. Documentation updates

- `architecture/search_backend.md`: current argument contract and compatibility aliases.
- `architecture/tool.md`: current model-facing search/evidence tool surface.
- `architecture/config.md`: backend/fallback/eggsearch configuration if affected.
- `README.md`: current eggsearch install/doctor guidance only if stale.
- relevant test documentation in `AGENTS.md` only if focused command names change.

## 13. Acceptance criteria

M001 is accepted only when all are true:

1. `websearch` and `webfetch` remain functional through eggsearch.
2. A representative current `repo_fetch` request contains separate `owner`, `repo`, and `path` and uses current range names.
3. A representative current `repo_map` request contains separate `owner` and `repo` and uses `max_depth`.
4. A legacy combined repo locator is either translated correctly or rejected clearly when ambiguous.
5. Security CVE filtering reaches eggsearch as `cve_id`; current GHSA/OSV/RustSec fields are available if exposed by the plan implementation.
6. `research_search` no longer forwards an undefined `domains` field without translation/validation.
7. Legacy `batch_fetch.urls` becomes valid tagged web items and current structured repo items contain required owner/repo/path fields.
8. `evidence_bundle` accepts current source-card/fetch evidence inputs rather than the stale pseudo-source format.
9. No currently exposed field is silently ignored solely because upstream renamed/removed it.
10. Focused fake MCP tests would fail if required current fields were changed back to the audited stale forms.
11. Default backend, raw-tool exposure, fallback, trust, timeout, and output-bound invariants remain unchanged.
12. `scripts/verify.sh quick` is green on the accepted implementation candidate.
13. No new provider client, CI lane, compatibility matrix, scheduled job, or release automation is introduced.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- current eggsearch 0.3.6 source/schema contradicts the audited contract in a way that changes ownership or tool semantics materially;
- preserving a legacy CodeGG argument would require inventing semantics not provided by eggsearch;
- a fix requires changing generic MCP protocol behavior for all servers rather than the search integration boundary;
- the implementation would require adding a second search backend or provider-specific network client;
- a durable storage migration unexpectedly becomes necessary;
- repository evidence shows the default backend/fallback policy has intentionally changed since the plan baseline;
- the work expands into CodeGG research synthesis, provider credentials, or browser automation beyond the explicit request-contract repair.

## 15. Closure evidence required

Create `plans/closure/search-eggsearch-integration/001-status.md` containing:

- implementation commit(s);
- exact accepted CodeGG revision;
- eggsearch contract baseline used (0.3.6 / `4ccb374...` unless newer audited during execution);
- a tool-by-tool CodeGG input -> upstream request matrix;
- explicit disposition of each audited defect;
- compatibility aliases retained and their semantics;
- focused test commands and outcomes;
- `scripts/verify.sh quick` outcome;
- documentation files updated;
- unresolved findings classified by severity;
- recommendation: closed, conditionally closed, corrective pass required, or blocked.

Closure must not claim real-binary compatibility evidence unless it was actually run. M003 owns the required real-process closure smoke.

## 16. Handoff notes

- Search backend tests mutate process-global state and already use cross-process locking; preserve the existing serialization discipline.
- Prefer pure normalization helpers so most contract tests do not require MCP process setup.
- Keep request translation centralized in `search_backend`; do not move provider semantics into each tool module.
- Use current eggsearch 0.3.6 source/schema as the interface authority, not stale CodeGG architecture prose.
- Preserve unrelated user changes.
- Do not broaden verification. The purpose is to repair a concrete contract gap and make the focused tests capable of detecting it.
