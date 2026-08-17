# Search and Eggsearch Integration M004 — Deep-Research Structured-Consumption Corrective Pass

Status: implemented

Repository baseline reviewed: `6a625f73368ceb34c7a89f3287045ae039346126`

Corrective planning branch at authorship: `agent/search-eggsearch-contract-repair`

Source corrective addendum:

- `plans/subsystems/search-eggsearch-integration-deep-research-corrective-addendum.md`

Source roadmap and historical closure evidence:

- `plans/subsystems/search-eggsearch-integration-roadmap.md`
- `plans/closure/search-eggsearch-integration/002-status.md`
- `plans/closure/search-eggsearch-integration/003-status.md`

Normative planning references:

- `plans/003-planning-process.md#2.4-milestone-implementation-plans`
- `plans/003-planning-process.md#7-corrective-passes`

Audited eggsearch baseline:

- package version: `0.3.6`
- release commit recorded by M001–M003: `4ccb374af00348bba75761f6bbd1e192d385a2b9`

## 1. Objective

Close the post-M003 consumer-path gap in CodeGG deep research without reopening the already-correct provider-ownership or generic MCP work.

The completed implementation must make the ordinary CodeGG research collector consume eggsearch's lossless structured result, understand the actual current research grouping shape, emit only valid upstream workflow values, and preserve structured evidence through the retained `codesearch` compatibility alias.

The required end state is:

```text
ResearchCoordinator
      |
      v
EggsearchSource
      |
      v
search_backend::dispatch_*_structured
      |
      +--> complete upstream serde_json::Value ----> SourceRecord conversion
      |
      `--> bounded/trust-framed output ------------> model/display compatibility only
```

The bounded display projection MUST NOT be the authoritative source for CodeGG's internal research conversion when a structured value is present.

## 2. Why this corrective pass exists

M002 successfully consolidated external search ownership into eggsearch. M003 successfully added structured MCP/search-backend retention and demonstrated wrapper-level compatibility with a real eggsearch 0.3.6 process.

A later repository review found that the new deep-research adapter still uses the pre-M003 string path and therefore bypasses the structured result it should consume.

### 2.1 Current research adapter reparses display text

`src/research/sources/eggsearch.rs` currently:

- calls `search_backend::dispatch_research_search()` or `dispatch_security_search()`;
- receives the bounded, trust-framed `String` projection;
- strips the outer frame with `payload_from_framed()`;
- reparses that projection with `serde_json::from_str()`;
- converts the parsed object into `SourceRecord`s.

This recreates the exact class of truncation/opaque-response problem M003 added the structured path to avoid.

### 2.2 Current research response shape is not consumed correctly

Eggsearch 0.3.6 `ResearchSearchResponse` returns grouped source cards under:

```text
groups[*].results[*]
```

Each group contains a classification, label, source-card `results`, and truncation/quality metadata.

The current CodeGG converter only looks for top-level arrays named:

- `sources`
- `papers`
- `results`
- `hits`
- `items`
- `vulns`

When none exists, it treats the whole top-level response object as one candidate result. Because that object has no top-level `url`, `source_from_result()` discards it. A successful current `research_search` response can therefore yield zero CodeGG sources without an upstream error.

### 2.3 CodeGG research modes are not eggsearch workflow values

The current adapter forwards internal CodeGG mode labels such as:

- `landscape`
- `library_evaluation`
- `api_investigation`
- `debugging`
- `spec_digest`
- `narrow_answer`

Eggsearch 0.3.6 research workflows are instead drawn from:

- `general`
- `architecture_decision`
- `api_evaluation`
- `library_comparison`
- `migration_planning`
- `security_review`
- `performance_investigation`
- `ecosystem_survey`

M004 must introduce an explicit semantic mapping rather than forwarding CodeGG enum names verbatim.

### 2.4 Existing real-process smoke does not cover this path

`tests/eggsearch_real_compat.rs` verifies that the CodeGG `ResearchSearchTool` can invoke eggsearch with a simple query. It does not pass through `ResearchCoordinator` / `EggsearchSource`, does not convert `groups[*].results`, and does not exercise CodeGG research-mode workflow mapping.

The M003 smoke remains useful wrapper compatibility evidence; this corrective pass adds the missing consumer-path evidence rather than replacing it.

### 2.5 `codesearch` structured execution still loses the retained value

`codesearch` is now correctly a compatibility alias over eggsearch `repo_search(profile = "coding")`, but `CodeSearchTool::execute_structured()` calls its string-returning `execute()` path and constructs provenance around the string. The upstream structured repo-search value is therefore not retained through this alias.

This is lower severity than the deep-research data-loss bug, but it is the same local integration boundary and should be corrected in the same bounded pass.

## 3. Invariants that MUST NOT regress

1. Eggsearch remains CodeGG's default external-search backend.
2. `fallback_to_builtin` remains false by default.
3. Raw `mcp__eggsearch__*` tools remain hidden by default.
4. No direct Exa/Tavily/Brave/SerpAPI/Kagi network client is reintroduced into CodeGG's normal search or research path.
5. `src/search/*` remains an explicit generic compatibility fallback only.
6. CodeGG owns research orchestration, budgeting, local-source collection, synthesis, persistence, and report generation; eggsearch owns external discovery/provider routing.
7. External evidence remains `external_untrusted` data.
8. The existing shared `McpService` lifecycle remains authoritative; do not spawn an eggsearch process per research run or tool call.
9. Model-facing output remains bounded/trust-framed.
10. Structured upstream values are retained as evidence metadata and MUST NOT cause automatic execution of upstream `next_actions`.
11. Existing persisted research artifacts remain readable; no storage migration is expected.
12. Verification remains change-specific and does not add CI lanes, scheduled jobs, compatibility matrices, source scanners, dependency bots, or release automation.

## 4. Explicit non-goals

M004 does not:

- redesign the research planner, synthesizer, claim model, persistence layer, or report renderer;
- mirror all eggsearch Rust response structs inside CodeGG;
- add a new generic evidence graph;
- auto-follow URLs or `next_actions`;
- add provider configuration or credentials to CodeGG;
- remove the explicit legacy builtin search fallback;
- broaden MCP protocol work beyond the structured result API already added by M003;
- add browser/PDF/cache parity to the CodeGG wrapper surface;
- change daemon/process ownership;
- create a new network-dependent CI test.

## 5. Required production-code changes

### Work package A — Make `EggsearchSource` consume structured search-backend results

Primary files:

- `src/research/sources/eggsearch.rs`
- `src/search_backend/mod.rs` only if a small visibility/helper adjustment is actually required

Required behavior:

1. `EggsearchSource::collect_external()` must call the existing structured dispatch surface:
   - `dispatch_research_search_structured()` for ordinary research modes;
   - `dispatch_security_search_structured()` for `ResearchMode::SecurityReview`.
2. Internal conversion must use `StructuredSearchResult.value` whenever present.
3. The model/display `output` string must no longer be reparsed when a structured value exists.
4. Preserve the current bounded/framed output for model-facing compatibility; this plan changes the internal consumer, not the display contract.
5. Older/text-only compatibility may remain only as an explicit degraded path:
   - if `value == None` and the display projection is not truncated, a legacy parse MAY be attempted to preserve compatibility;
   - if `value == None` and the projection is truncated, return a bounded `ResearchError::SourceCollection` explaining that structured evidence is unavailable rather than silently producing zero sources or reparsing invalid JSON.
6. Do not add another cache or intermediate persistence representation.

Acceptance for package A:

- a test can deliberately cap/truncate model-facing output while retaining a complete structured response, and research conversion still returns the expected sources;
- no current successful structured result depends on `payload_from_framed()` for normal conversion.

### Work package B — Convert current grouped research source cards correctly

Primary file:

- `src/research/sources/eggsearch.rs`

Required behavior:

1. Recognize current `ResearchSearchResponse.groups`.
2. Iterate every group object's `results` array in stable response order.
3. Convert each valid SourceCard-like object into a `SourceRecord` using the existing narrow CodeGG model.
4. Continue rejecting non-HTTP(S) external URLs.
5. Preserve useful upstream provenance without expanding the storage schema unnecessarily:
   - source remains identified as eggsearch;
   - trust remains `external_untrusted`;
   - provider, source kind/class, stable source ID, publication timestamp, and useful snippet/title metadata should be retained where the existing `SourceRecord` fields/notes can represent them;
   - do not replace CodeGG's local `SourceRecord.id` contract with an upstream ID unless the existing research domain model explicitly permits that.
6. A group with zero results is not an error.
7. A response with source cards present must not silently convert to an empty vector.
8. Unknown additive top-level/group/source-card fields must be ignored/preserved safely rather than causing failure.

Implementation guidance:

- Prefer small `serde_json::Value` helpers over introducing a full CodeGG copy of eggsearch's research structs.
- A helper such as `research_result_items(value)` MAY flatten `groups[*].results` and support one documented legacy shape if needed, but current grouped semantics are authoritative.

Acceptance for package B:

- a fixture containing at least two groups and at least three source cards converts all valid cards;
- stable response order is preserved;
- invalid/non-HTTP(S) cards are filtered without dropping valid siblings;
- provider/trust/stable-id provenance survives conversion in the existing representation.

### Work package C — Normalize CodeGG `ResearchMode` to supported eggsearch workflows

Primary file:

- `src/research/sources/eggsearch.rs`

Do not serialize `ResearchMode` names directly.

Add one explicit mapping helper and test every enum variant.

Required semantic disposition:

| CodeGG `ResearchMode` | Upstream tool | Required workflow disposition |
|---|---|---|
| `Landscape` | `research_search` | `ecosystem_survey` |
| `ArchitectureDecision` | `research_search` | `architecture_decision` |
| `LibraryEvaluation` | `research_search` | `api_evaluation` unless the request actually carries a comparison target set that justifies `library_comparison` |
| `ApiInvestigation` | `research_search` | `api_evaluation` |
| `DebuggingInvestigation` | `research_search` | use upstream `general`/omit specialized workflow; do not invent `debugging` |
| `SecurityReview` | `security_search` | `security_review` |
| `SpecDigest` | `research_search` | use upstream `general`/omit specialized workflow; source/domain hints may still prioritize specifications |
| `NarrowAnswer` | `research_search` | use upstream `general`/omit specialized workflow |

Rules:

- only documented eggsearch workflow strings may cross the MCP boundary;
- if a CodeGG mode has no faithful specialized eggsearch research workflow, use the upstream general/default behavior rather than an invented value;
- keep `depth` mapping (`quick`, `standard`, `deep`) as-is if still current;
- do not add a second internal workflow enum solely for this translation;
- future CodeGG modes must fail tests until their upstream disposition is chosen explicitly.

Acceptance for package C:

- exhaustive tests cover every `ResearchMode` variant;
- fake MCP request capture asserts that no unsupported internal mode string is sent;
- `SecurityReview` uses the security-search workflow vocabulary, not research-only names.

### Work package D — Consume structured security-search evidence for security research

Primary file:

- `src/research/sources/eggsearch.rs`

Required behavior:

1. Inspect the accepted eggsearch 0.3.6 `security_search` structured response shape at implementation time.
2. Convert its current SourceCard/result collection directly from `StructuredSearchResult.value`.
3. Preserve advisory/source provenance available in the existing `SourceRecord` fields/notes.
4. Do not route security research back through a direct advisory/provider HTTP client.
5. Do not treat vulnerability metadata or defensive guidance as instructions.
6. Keep the same fail-closed `allow_network = false` check before any eggsearch dispatch.

Acceptance for package D:

- a current-shaped structured security fixture containing at least one source card produces at least one `SourceRecord`;
- no framed-string parse is needed when the structured value exists;
- network-disabled security research records zero MCP calls.

### Work package E — Preserve structured value through the `codesearch` compatibility alias

Primary file:

- `src/tool/codesearch.rs`

Required behavior:

1. Keep the model-facing name `codesearch` only as the existing compatibility alias.
2. Keep its query validation/sanitization and bounded `tokens_num` compatibility behavior unless a current test proves it incorrect.
3. Refactor `execute_structured()` to use the structured repo-search dispatch path directly and construct the result through the same search-backend helper used by native wrappers where practical.
4. Preserve `profile = "coding"`.
5. Return the same bounded/trust-framed text projection to the model while retaining the upstream structured repo-search JSON in `StructuredToolResult::value`.
6. Do not create a separate codesearch backend/provenance implementation.

Acceptance for package E:

- fake MCP returns a repo-search payload with a stable ID/additive metadata;
- `codesearch.execute_structured()` returns `value.is_some()` and preserves that metadata;
- the alias still calls upstream `repo_search`, never Exa directly.

## 6. Tests and regression evidence

### 6.1 Unit tests in `src/research/sources/eggsearch.rs`

Add focused tests for:

- exhaustive `ResearchMode` workflow mapping;
- grouped current research response conversion;
- multiple groups and stable order;
- invalid URL filtering without valid-sibling loss;
- upstream provider/stable-id/trust/source-kind provenance retention;
- structured value preferred over conflicting/truncated display text;
- text-only compatibility behavior if retained;
- truncated text-only response fails explicitly instead of silently returning empty evidence.

### 6.2 Fake MCP consumer-path integration

Extend the smallest existing fake-MCP integration surface or add one narrow test file if that is clearer.

The regression MUST exercise more than `ResearchSearchTool` directly. It must pass through the CodeGG research collector/coordinator boundary sufficiently to prove:

```text
current-shaped fake eggsearch response
        -> structured dispatch
        -> EggsearchSource
        -> SourceRecord collection
```

Required assertions:

- `allow_network = false` performs zero MCP calls;
- a normal research request sends only an accepted workflow value;
- a grouped structured response produces non-zero expected sources;
- a deliberately small display cap does not prevent source conversion from the structured value;
- security-review mode calls `security_search`, not `research_search`;
- security structured evidence is converted.

Do not add a new cross-process locking scheme merely for this test. Reuse existing search-backend test support where needed.

### 6.3 `codesearch` regression

Add or extend a focused tool/fake-MCP test proving:

- tool name remains `codesearch`;
- upstream call remains `repo_search` with `profile = "coding"`;
- structured upstream value survives `execute_structured()`.

### 6.4 Real-process compatibility

The existing M003 opt-in eggsearch 0.3.6 smoke remains accepted evidence that the wrappers reach current handlers.

M004 does not require a new network-dependent CI lane or version matrix.

If a locally runnable eggsearch 0.3.6/current binary is readily available during implementation, one optional local smoke MAY exercise a representative research mode through the collector. Failure to have such a binary is not a blocker if the deterministic current-shaped consumer regression is complete and `scripts/verify.sh quick` is green.

## 7. Documentation updates

Update only documentation made inaccurate by the corrective implementation.

At minimum review:

- `architecture/research.md`
- `architecture/search_backend.md`
- `architecture/tool.md` if `codesearch` structured behavior is described
- `.opencode/skills/...` search/research skill documentation only if it currently states the wrong consumer behavior

Documentation must state:

- eggsearch structured values are the authoritative internal evidence source for deep research;
- bounded trust-framed strings are compatibility/model projections, not the source for internal parsing;
- current grouped research source cards are consumed through `groups[*].results`;
- CodeGG research modes are translated onto supported eggsearch workflows rather than forwarded by enum name.

Do not add user-facing provider credential instructions.

## 8. Storage, protocol, migration, and compatibility effects

### Storage

No schema migration is expected.

Existing `SourceRecord` persistence remains authoritative. Preserve upstream stable IDs/provider metadata in existing fields or notes where possible rather than adding a new database column solely for this correction.

If durable schema changes prove necessary to satisfy a required acceptance criterion, stop and create a separate migration plan.

### Protocol

No public CodeGG protocol change is required.

The existing additive structured MCP call surface from M003 remains the transport boundary.

### Compatibility

- current eggsearch structured responses are authoritative;
- text-only older-server behavior may remain as explicit degraded compatibility, but must not silently treat truncated text as complete evidence;
- existing CodeGG research modes and `codesearch` tool name remain available;
- no unsupported internal workflow labels may be forwarded to eggsearch.

## 9. Security and trust review

Before closure verify:

- structured external values remain marked/treated as external untrusted evidence;
- no upstream `next_actions`, snippets, advisory text, or source-card text is interpreted as CodeGG control instructions;
- external URLs remain HTTP(S)-validated before becoming source locators;
- provider credentials remain owned by eggsearch and are not copied into CodeGG errors/notes;
- security/advisory metadata is preserved as evidence only;
- no direct provider HTTP path was reintroduced.

## 10. Ordered implementation sequence

1. Re-read current `src/research/sources/eggsearch.rs`, `ResearchSourceAdapter`, `ResearchCoordinator`, and `StructuredSearchResult` APIs on the implementation baseline.
2. Confirm the accepted eggsearch 0.3.6/current research and security structured response shapes from upstream source/schema before editing conversion code.
3. Add the explicit `ResearchMode` -> upstream tool/workflow mapping helper and exhaustive tests.
4. Refactor `EggsearchSource::collect_external()` onto structured dispatch.
5. Implement current grouped research source-card flattening and provenance conversion.
6. Implement current security structured-result conversion through the same narrow source-record boundary.
7. Add text-only degraded compatibility behavior only if needed; fail explicitly on truncated text-only evidence.
8. Refactor `codesearch.execute_structured()` to retain the repo-search value.
9. Add the consumer-path fake-MCP regression and focused alias/security tests.
10. Update only affected architecture/skill documentation.
11. Run focused verification.
12. Run `scripts/verify.sh quick`.
13. Create `plans/closure/search-eggsearch-integration/004-status.md` only after all acceptance criteria are demonstrated.
14. Move M004 out of dependency-ready registry state and return the corrective addendum/search subsystem to closed only after that closure record is accepted.

## 11. Verification commands

Use repository-native wrappers such as `rtk` where the current repository process requires them; command spellings below describe the intended checks.

Focused first:

```bash
cargo test --lib research::sources::eggsearch -- --test-threads=1
cargo test --test fake_eggsearch_mcp -- --test-threads=1
```

If a dedicated research integration test file is added, run that file explicitly as well.

Run the focused tool test that owns `codesearch` structured execution.

Then:

```bash
cargo fmt --all -- --check
git diff --check
scripts/verify.sh quick
```

Do not default to `scripts/verify.sh full`, a network smoke, or a workspace version matrix. Escalate only if a focused or quick verification failure cannot be classified locally.

## 12. Static/source guards

No new permanent source scanner is required.

Final-tree inspection must nevertheless confirm:

- `EggsearchSource` normal structured path calls `dispatch_*_structured`;
- normal structured conversion does not call `payload_from_framed()`;
- no CodeGG `ResearchMode` debug/name string is blindly serialized to eggsearch workflow;
- `codesearch` still contains no Exa endpoint/API-key handling;
- no direct Tavily/Brave/SerpAPI/Kagi client was reintroduced;
- legacy `src/search/*` remains explicit fallback-only.

These may be closure-review checks rather than new CI guards.

## 13. Acceptance criteria

M004 is complete only when all are true:

1. A current eggsearch research response with source cards under `groups[*].results` produces the expected non-zero `Vec<SourceRecord>`.
2. Every valid source card across multiple groups is considered in stable response order.
3. Normal deep-research conversion uses the complete structured value, not the bounded/trust-framed string projection.
4. Deliberately truncating the display projection does not lose structured research evidence.
5. If a text-only degraded compatibility path remains, a truncated text-only response fails clearly rather than silently yielding empty/partial evidence as complete.
6. Every `ResearchMode` has an explicit tested upstream workflow disposition, and only documented eggsearch workflow strings are sent.
7. `SecurityReview` uses `security_search` with an accepted security workflow and converts structured security source evidence.
8. `allow_network = false` prevents all external eggsearch research/security calls.
9. `codesearch.execute_structured()` preserves upstream repo-search JSON in `StructuredToolResult::value` while keeping the existing bounded model-facing output and coding profile.
10. No direct provider HTTP client or credential path is reintroduced.
11. Existing default-backend/raw-tool/fallback/trust invariants remain unchanged.
12. Focused tests are green.
13. `cargo fmt --all -- --check` and `git diff --check` are green.
14. `scripts/verify.sh quick` is green.
15. Documentation accurately describes the structured deep-research consumer path.
16. A closure record documents the exact implementation revision, tests, remaining limitations, and final disposition.

No criterion may be satisfied solely by the raw `ResearchSearchTool` smoke; the CodeGG research consumer path must be covered.

## 14. Stop conditions

Stop and report rather than widening this plan if any of the following occurs:

- current eggsearch has moved to a materially breaking response contract beyond the documented 0.3.6/additive compatibility assumptions;
- satisfying source identity/provenance requires a durable CodeGG storage migration;
- the correction requires changing claim/synthesis semantics rather than external source collection;
- the generic MCP structured API is insufficient and would require a broad cross-server redesign;
- the explicit legacy builtin fallback must be deleted to make the change work;
- a new provider-specific client or credential path appears necessary.

Any such finding requires a separate focused plan or an explicit update to this corrective addendum before implementation proceeds.

## 15. Closure evidence required

`plans/closure/search-eggsearch-integration/004-status.md` must include:

- implementation commit(s);
- exact accepted repository revision;
- requirement-to-evidence matrix for all sixteen acceptance criteria;
- current research/security response shapes consumed;
- captured/tested workflow mapping table;
- consumer-path regression results proving `groups[*].results -> SourceRecord`;
- truncation-vs-structured-value regression evidence;
- network-disabled zero-call evidence;
- `codesearch` structured-value evidence;
- focused test outputs;
- `scripts/verify.sh quick` result;
- final-tree ownership/security review;
- unresolved findings by severity;
- recommendation: closed, conditionally closed, corrective pass required, or blocked.

M003's existing closure record must not be rewritten to hide this later-discovered defect. M004 closure supersedes only the current strict subsystem disposition while retaining M001–M003 as historical evidence.
