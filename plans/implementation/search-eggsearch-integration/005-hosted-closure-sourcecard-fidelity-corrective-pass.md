# Search and Eggsearch Integration M005 — Hosted Closure and SourceCard Fidelity Corrective Pass

Status: active

Repository baseline reviewed: `77c10b98342777381f295aa1b16bb3d44999ae12`

Planning branch: `agent/search-eggsearch-contract-repair`

Source corrective addendum:

- `plans/subsystems/search-eggsearch-integration-hosted-closure-sourcecard-fidelity-corrective-addendum.md`

Source roadmap and predecessor evidence:

- `plans/subsystems/search-eggsearch-integration-roadmap.md`
- `plans/closure/search-eggsearch-integration/003-status.md`
- `plans/closure/search-eggsearch-integration/004-status.md`

Normative planning references:

- `plans/003-planning-process.md#2.4-milestone-implementation-plans`
- `plans/003-planning-process.md#2.5-closure-records`
- `plans/003-planning-process.md#7-corrective-passes`

Audited eggsearch baseline:

- package version: `0.3.6`
- current canonical `SourceCard` fields include `providers: Vec<String>` and nested `metadata.source_kind`
- current research workflows include `library_comparison`, `api_evaluation`, `ecosystem_survey`, `architecture_decision`, `security_review`, and `general`

Hosted failure evidence:

- PR: `#78`
- failed exact head: `77c10b98342777381f295aa1b16bb3d44999ae12`
- workflow run: `31930352527`
- verify job: `95124064959`
- failing step: Workspace Clippy
- diagnostic: `src/research/sources/eggsearch.rs:92`, `clippy::type_complexity` on `result_items()` return type
- Workspace tests: skipped after Clippy failure

## 1. Objective

Finish the search/eggsearch corrective workstream with one small evidence-driven pass.

M005 must preserve M004's functional correction while addressing the exact defects discovered after M004 was marked closed:

1. make the branch pass the repository's existing hosted Clippy/test gate on the exact final candidate;
2. make CodeGG's research converter consume canonical eggsearch 0.3.6 `SourceCard` provenance fields rather than fixture-only aliases;
3. correct the semantic workflow mapping for `ResearchMode::LibraryEvaluation`;
4. reconcile PR, registry, roadmap/addendum, and closure state only after the exact final candidate is green.

This is not a new search architecture phase. It is a bounded correctness and closure pass.

## 2. Current implementation evidence

### 2.1 M004 behavior that must remain intact

`src/research/sources/eggsearch.rs` currently:

- routes normal external research through `dispatch_research_search_structured()`;
- routes `SecurityReview` through `dispatch_security_search_structured()`;
- consumes `StructuredSearchResult.value` before considering bounded display output;
- rejects truncated text-only fallback when no structured value exists;
- traverses current grouped research responses under `groups[*].results`;
- filters non-HTTP(S) external URLs;
- keeps network-disabled requests fail-closed;
- maps CodeGG modes to accepted eggsearch workflow strings.

`src/tool/codesearch.rs` currently:

- remains a compatibility alias over eggsearch `repo_search` with `profile = "coding"`;
- uses `dispatch_repo_search_structured()` in `execute_structured()`;
- preserves the upstream structured value through the normal `StructuredToolResult` helper.

These are accepted M004 implementation results. M005 MUST NOT replace them with string parsing or direct provider calls.

### 2.2 Exact hosted verification failure

The M004 closure head `77c10b98342777381f295aa1b16bb3d44999ae12` failed the ordinary PR `CI / verify` workflow.

The failure is directly attributable to M004 code:

```text
error: very complex type used. Consider factoring parts into `type` definitions
  --> src/research/sources/eggsearch.rs:92:41

fn result_items(payload: &Value)
    -> Vec<(&Map<String, Value>, Option<&Map<String, Value>>)> {
       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
```

The repository runs Clippy with `-D warnings`; therefore this is a hard merge/closure defect, not optional polish.

Do not solve this by weakening CI or adding a blanket lint allow. Prefer a local type alias or helper type/decomposition that keeps the converter readable.

### 2.3 Current SourceCard fidelity gap

The M004 converter currently looks for singular/top-level provenance fields such as:

- `provider` / `source`;
- `source_type` / `source_kind` / `type` / `kind`.

The M004 fixture similarly emits synthetic fields such as `provider: "arxiv"` and `source_type: "paper"`.

Current eggsearch 0.3.6 `SourceCard` instead serializes approximately as:

```json
{
  "id": "src_...",
  "stable_id": "src_...",
  "title": "...",
  "url": "https://...",
  "snippet": "...",
  "providers": ["arxiv", "openalex"],
  "score": 0.81,
  "trust": "external_untrusted",
  "fetched": false,
  "trust_markers": {},
  "metadata": {
    "source_kind": "official_docs"
  }
}
```

The exact optional metadata fields may vary, but `providers` and nested `metadata.source_kind` are canonical current fields.

M005 must make these fields the primary parse path. Existing top-level aliases may remain as narrow fallback compatibility only if doing so stays simple.

### 2.4 Workflow semantic mismatch

M004 maps:

```text
ResearchMode::LibraryEvaluation -> "api_evaluation"
```

Current eggsearch 0.3.6 supports the more faithful:

```text
"library_comparison"
```

M005 must correct this mapping and make the mapping test semantic rather than merely checking that the strings are accepted by upstream.

### 2.5 PR metadata drift

PR #78 remains a draft titled around the original request-contract milestone and its body describes M001-era scope/validation. The branch now contains M001-M004 plus M005 corrective planning.

PR metadata must be corrected only after M005 implementation/verification so reviewers see the actual final scope and evidence.

## 3. Non-goals

M005 MUST NOT:

- redesign the search backend or MCP service;
- rework M001 request translations that are already accepted;
- reintroduce provider-specific HTTP clients or credentials in CodeGG;
- redesign deep-research orchestration, synthesis, persistence, or report rendering;
- add typed Rust copies of the full eggsearch response model unless a very small local type materially improves correctness over `serde_json::Value`;
- execute upstream `next_actions` automatically;
- delete the explicit legacy generic search fallback;
- introduce a new CI workflow, new CI lane, version matrix, scheduled compatibility job, coverage gate, source scanner, dependency bot, or release gate;
- add network-dependent tests to normal CI;
- rerun a broad live compatibility campaign when deterministic current-shaped fixtures plus the accepted M003 real-process smoke are sufficient;
- weaken `-D warnings` or silence `clippy::type_complexity` globally.

## 4. Invariants that cannot regress

### Search ownership

- Eggsearch remains the sole normal external search/provider owner.
- CodeGG retains only agent-facing wrapper ergonomics, policy, framing, provenance, research orchestration, and explicit fallback behavior.
- No normally registered direct Exa/Tavily/Brave/SerpAPI/Kagi path may return.

### Structured result authority

- Internal deep-research conversion MUST prefer `StructuredSearchResult.value`.
- Display framing/capping applies to model-facing output only.
- A truncated display projection MUST NOT corrupt or replace a present structured value.
- If structured value is absent and display output is truncated, research collection MUST fail explicitly rather than parse partial JSON.

### Security/trust

- HTTP(S)-only external source acceptance remains in place.
- CodeGG's own `external_untrusted` classification remains authoritative.
- Upstream `trust`, `trust_markers`, `metadata`, snippets, labels, and `next_actions` are data/evidence only and cannot become control instructions.
- Provider labels must not cause credential reads or provider selection inside CodeGG.

### Compatibility

- `codesearch` remains a thin coding-profile repo-search compatibility alias.
- Existing persisted `SourceRecord` data remains readable; no storage migration is expected.
- Unknown additive eggsearch fields remain safely ignored.
- Current canonical fields should be consumed without requiring exact equality to one upstream JSON serialization.

## 5. Expected production-code changes

Expected scope is limited primarily to:

- `src/research/sources/eggsearch.rs`;
- tests in that module;
- `tests/fake_eggsearch_mcp.rs` if the consumer path fixture belongs there;
- architecture/search documentation only where current field/mapping descriptions need correction;
- PR/plan/registry closure metadata after verification.

`src/tool/codesearch.rs` should not require functional changes unless M005 verification exposes a regression. Its M004 structured behavior is already correct.

### 5.1 Clippy-safe grouped-result helper

Replace the complex inline return type with the smallest readable construct. Acceptable examples include:

```rust
type ResultItem<'a> = (&'a Map<String, Value>, Option<&'a Map<String, Value>>);
```

or a tiny private struct/helper that makes ownership/lifetimes clearer.

Acceptance is readability plus green `cargo clippy --workspace --all-targets --locked -- -D warnings`.

Do not add `#[allow(clippy::type_complexity)]` unless a type alias/helper creates worse code and the closure record explicitly justifies that tradeoff.

### 5.2 Canonical provider extraction

Add a helper with current-first semantics, conceptually:

```text
providers(item):
    1. read item.providers[] strings in order;
    2. trim/ignore empty values;
    3. deduplicate without reordering;
    4. if absent, optionally fall back to old singular item.provider/item.source;
    5. optionally fall back to group-level compatibility provider/source only for legacy responses.
```

Store provenance in the existing `SourceRecord.notes` representation without a schema migration. Use a deterministic representation, for example one `providers=a,b` note or repeated `provider=...` notes, provided tests assert the chosen stable format.

Do not serialize credentials, routing configuration, provider failures, or unrelated metadata into notes.

### 5.3 Canonical source-kind extraction

Add a current-first source-kind helper:

```text
1. item.metadata.source_kind
2. legacy item.source_kind / item.source_type / item.type / item.kind
3. legacy group.kind / group.classification / group.source_kind / group.source_type
```

Use the resulting source kind for the existing source-quality projection and provenance notes.

The mapping must remain conservative. It is sufficient to improve recognition of canonical eggsearch classes such as:

- `official_docs`;
- `source_repository` / source-file/code-related kinds;
- `security_advisory`;
- `reference`;
- other current enum values where the existing `SourceQuality` vocabulary has a clear equivalent.

Do not invent a new persistent quality taxonomy in M005.

### 5.4 Trust-field handling

Current-shaped fixtures must include upstream `trust` and `fetched` fields so they exercise realistic SourceCard serialization.

The converter MAY retain the upstream trust label as diagnostic provenance if useful, but it MUST NOT use it to elevate CodeGG trust. The CodeGG source remains externally untrusted.

### 5.5 Workflow mapping

Update the mapping to:

```rust
ResearchMode::Landscape => "ecosystem_survey",
ResearchMode::ArchitectureDecision => "architecture_decision",
ResearchMode::LibraryEvaluation => "library_comparison",
ResearchMode::ApiInvestigation => "api_evaluation",
ResearchMode::DebuggingInvestigation => "general",
ResearchMode::SecurityReview => "security_review",
ResearchMode::SpecDigest => "general",
ResearchMode::NarrowAnswer => "general",
```

The test must cover all variants and specifically assert the distinction between `LibraryEvaluation` and `ApiInvestigation`.

## 6. Ordered work packages

### WP1 — Reproduce and clear the exact lint failure

1. Inspect the current `result_items()` helper and all call sites.
2. Replace the complex return signature with a private alias/helper decomposition.
3. Run formatting and the exact workspace Clippy command.
4. Do not continue to closure if Clippy remains red.

Deliverable: no lint suppression debt and the exact hosted failure class is removed locally.

### WP2 — Make SourceCard extraction current-first

1. Add provider-array extraction from `providers`.
2. Add nested source-kind extraction from `metadata.source_kind`.
3. Preserve narrow legacy fallbacks only where unambiguous.
4. Keep stable ID, title, URL, snippet, publication parsing, URL filtering, and structured-value authority unchanged.
5. Confirm no new dependency or schema change is needed.

Deliverable: real eggsearch cards retain provider/source-kind provenance through `SourceRecord`.

### WP3 — Replace synthetic fixtures with canonical current-shaped fixtures

At minimum, one grouped research fixture must contain multiple current-shaped cards and exercise:

- `id`;
- `stable_id`;
- `title`;
- `url`;
- `snippet`;
- `providers` with at least two entries on one card;
- `trust: "external_untrusted"`;
- `fetched: false`;
- `metadata.source_kind`;
- an unknown additive field that is ignored safely.

Retain one invalid/non-HTTP(S) sibling to verify filtering without losing valid cards.

The integration/fake-MCP fixture used for truncation resistance should also use canonical current fields rather than the old synthetic singular provider/source-type fields.

Security fixture should use a current-shaped SourceCard appropriate to an advisory result, including `metadata.source_kind = "security_advisory"` where current upstream shape permits it.

Deliverable: the regression suite can no longer pass solely against response objects that eggsearch 0.3.6 would not normally serialize.

### WP4 — Correct workflow semantic mapping

1. Change `LibraryEvaluation` to `library_comparison`.
2. Keep `ApiInvestigation` as `api_evaluation`.
3. Update exhaustive mapping tests.
4. Update fake-MCP request capture if it asserts the affected mode.
5. Do not broaden into automatic `compare_targets` inference unless an existing CodeGG request field has a clear direct mapping; that is outside this corrective need.

Deliverable: CodeGG emits the most faithful supported workflow for library evaluation.

### WP5 — Focused regression verification

Run, at minimum:

```bash
cargo fmt --all -- --check
git diff --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --lib research::sources::eggsearch -- --test-threads=1
cargo test --test fake_eggsearch_mcp -- --test-threads=1
cargo test --test search_backend_eggsearch -- --test-threads=1
cargo test --test search_backend_arg_mapping -- --test-threads=1
scripts/verify.sh quick
```

`search_backend_legacy` should be run if touched or if `scripts/verify.sh quick` does not already give equivalent confidence for fallback preservation.

Do not add more verification commands merely to create evidence volume.

Deliverable: focused/local gates green.

### WP6 — Exact hosted closure evidence

1. Push the implementation candidate to the existing PR branch.
2. Let the ordinary existing PR `CI / verify` run on that exact candidate/merge ref.
3. Require Workspace Clippy to pass.
4. Require Workspace tests to execute and pass.
5. Record exact run ID, job ID, candidate SHA, and conclusion.
6. If hosted failure is caused by M005 code, keep M005 active and fix it.
7. If hosted failure is genuinely unrelated, classify the evidence precisely rather than claiming strict closure; do not create new CI machinery to work around it.

Deliverable: exact-candidate hosted evidence adequate for the chosen closure disposition.

### WP7 — PR and planning reconciliation

Only after implementation and verification:

1. update PR #78 title/body to describe the complete M001-M005 search/eggsearch integration correction and current validation;
2. keep the PR draft if further review is still desired; draft/ready status is a review decision, not an implementation acceptance criterion unless repository practice says otherwise;
3. create `plans/closure/search-eggsearch-integration/005-status.md`;
4. record M004 as historical closed implementation evidence whose current strict disposition was superseded by M005 after exact hosted failure and fidelity review;
5. mark the M005 addendum/search subsystem closed only if M005 closure evidence supports it;
6. remove M005 from dependency-ready work when closed;
7. do not alter unrelated subsystem statuses.

Deliverable: repository planning state matches actual evidence.

## 7. Required tests and assertions

### Unit: exact workflow mapping

Assert all eight CodeGG modes and explicitly assert:

```text
LibraryEvaluation != ApiInvestigation
LibraryEvaluation -> library_comparison
ApiInvestigation -> api_evaluation
```

### Unit: current-shaped grouped SourceCard conversion

Given:

```json
{
  "groups": [
    {
      "kind": "official_docs",
      "label": "Documentation",
      "results": [
        {
          "id": "src_runtime",
          "stable_id": "src_deadbeef",
          "title": "Runtime docs",
          "url": "https://example.com/docs",
          "snippet": "Reference",
          "providers": ["duckduckgo", "mojeek"],
          "score": 0.75,
          "trust": "external_untrusted",
          "fetched": false,
          "trust_markers": {},
          "metadata": {"source_kind": "official_docs"},
          "future_field": {"ignored": true}
        }
      ],
      "truncated": false
    }
  ]
}
```

assert:

- one `SourceRecord` is produced;
- URI/title/snippet are retained;
- stable ID is retained in existing provenance representation;
- both providers are retained deterministically;
- canonical nested `metadata.source_kind` is retained/used;
- CodeGG trust remains external-untrusted;
- unknown additive fields do not fail conversion.

### Unit/integration: structured value survives display cap

Use current-shaped grouped cards and a deliberately tiny model-output cap. Assert the consumer still sees all structured cards even when the display projection is truncated.

### Unit/integration: text-only truncation still fails

Preserve the M004 regression: `value = None` plus `truncated = true` must fail with a bounded source-collection error.

### Integration: security current-shaped SourceCard

For `SecurityReview`:

- only `security_search` is called;
- workflow is `security_review`;
- current-shaped advisory SourceCard converts;
- provider/source-kind provenance is retained;
- network-disabled request still fails before MCP dispatch.

### Integration: codesearch regression

Preserve the M004 test proving `codesearch.execute_structured()` carries the repo-search structured value and coding profile. No new codesearch behavior is required.

## 8. Static guards and documentation

No new permanent static guard is required for provider/source-card shape.

The existing compile/lint/test surface is sufficient if fixtures are accurate.

Update documentation only where it would otherwise become false, especially:

- `architecture/research.md` if it describes provider/source-kind conversion;
- `architecture/search_backend.md` if it states current structured-response fields;
- `architecture/tool.md` only if PR/final tool behavior description changes.

Do not add a separate compatibility document, CI job, or generated schema snapshot solely for M005.

## 9. Storage, protocol, migration, and compatibility effects

### Storage

No migration expected. Continue using existing `SourceRecord` fields/notes.

If retaining canonical provider/source-kind provenance cannot be done without changing durable storage, stop and create a separate migration plan rather than silently expanding M005.

### Protocol

No CodeGG public protocol change expected.

M005 only consumes more accurately the structured JSON already returned through M003's search-backend/MCP path.

### Upstream compatibility

Canonical current fields are primary. Narrow aliases may remain to tolerate older/text-only results.

Unknown additive upstream fields must remain non-fatal.

Do not hard-code exact equality to every current optional SourceCard field.

### Security

No trust elevation. `external_untrusted` remains CodeGG policy even if upstream sends another trust string.

No provider credentials or routing secrets enter CodeGG research notes.

## 10. Acceptance criteria

M005 is complete only when every criterion below is satisfied.

### Build and lint

- [ ] `cargo fmt --all -- --check` passes.
- [ ] `git diff --check` passes.
- [ ] `cargo clippy --workspace --all-targets --locked -- -D warnings` passes without a new global/local blanket lint suppression for this issue.
- [ ] the specific M004 `type_complexity` failure is absent.

### SourceCard fidelity

- [ ] canonical `providers: Vec<String>` is consumed before singular compatibility aliases;
- [ ] canonical `metadata.source_kind` is consumed before top-level compatibility aliases;
- [ ] provider entries are retained deterministically without credential/config leakage;
- [ ] canonical source kind informs existing CodeGG quality/provenance where representable;
- [ ] fixture includes `trust` and `fetched` and proves those fields do not elevate CodeGG trust;
- [ ] unknown additive SourceCard fields are tolerated;
- [ ] old compatibility parsing, if retained, remains secondary and bounded.

### Workflow semantics

- [ ] `LibraryEvaluation -> library_comparison`;
- [ ] `ApiInvestigation -> api_evaluation`;
- [ ] all other M004 mappings remain intentional and tested;
- [ ] no unsupported workflow string crosses MCP.

### M004 regression preservation

- [ ] grouped `groups[*].results` conversion remains ordered;
- [ ] structured value remains authoritative over bounded display output;
- [ ] text-only truncated fallback fails explicitly;
- [ ] security review uses structured `security_search`;
- [ ] network-disabled research remains fail-closed;
- [ ] `codesearch` structured value retention remains green;
- [ ] no direct external provider client returns.

### Verification and hosted evidence

- [ ] focused tests pass;
- [ ] `scripts/verify.sh quick` passes;
- [ ] ordinary existing PR CI runs on the exact final candidate;
- [ ] hosted Workspace Clippy passes;
- [ ] hosted Workspace tests execute and pass;
- [ ] exact candidate SHA, run ID, and job ID are recorded in M005 closure evidence;
- [ ] no new CI lane/matrix/scheduled job/release gate is added.

### Planning and PR hygiene

- [ ] PR #78 title/body reflects M001-M005 final scope and validation;
- [ ] `plans/closure/search-eggsearch-integration/005-status.md` exists;
- [ ] M004 record is not rewritten to hide the later failure;
- [ ] registry/addendum status reflects the actual M005 disposition;
- [ ] no unrelated plan status is changed.

## 11. Stop conditions

Stop and report rather than silently broadening M005 if any of the following occurs:

- current eggsearch has moved materially beyond the audited 0.3.6 SourceCard/workflow contract and the required adaptation is no longer small;
- accurate provenance requires a durable `SourceRecord` schema migration;
- a broad generic MCP refactor appears necessary;
- the hosted failure reveals unrelated systemic CI/toolchain work beyond the local lint/code shape;
- fixing library evaluation correctly requires redesigning CodeGG research request semantics rather than selecting the supported upstream workflow;
- a new provider client, credential path, background worker, or network CI requirement appears necessary.

In these cases preserve completed M001-M004 work, record the new evidence, and create a separate plan only for the newly demonstrated boundary.

## 12. Closure evidence required

`plans/closure/search-eggsearch-integration/005-status.md` MUST contain:

- implementation commit SHA(s);
- final accepted candidate SHA;
- exact PR #78 hosted workflow run ID and verify job ID;
- confirmation that Workspace Clippy and Workspace tests passed on that candidate;
- focused test commands and outcomes;
- requirement-to-evidence matrix for Clippy, SourceCard providers/source kind, library workflow mapping, truncation, security, network budget, and codesearch;
- final compatibility notes against eggsearch 0.3.6;
- confirmation that CodeGG trust remains external-untrusted;
- confirmation that no provider client/credential ownership returned;
- documentation/PR metadata updates;
- unresolved findings by severity;
- recommendation: `closed`, `conditionally closed`, `corrective pass required`, or `blocked`.

Strict closure requires the exact hosted verification to be green through Workspace tests. A local-only green result is insufficient for strict M005 closure because the corrective trigger includes a failed exact-head hosted gate.
