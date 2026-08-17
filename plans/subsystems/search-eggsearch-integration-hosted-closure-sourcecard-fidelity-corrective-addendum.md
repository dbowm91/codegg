# Search and Eggsearch Integration — Hosted Closure and SourceCard Fidelity Corrective Addendum

Status: active

Source roadmap:

- `plans/subsystems/search-eggsearch-integration-roadmap.md`

Historical closure records retained without revision:

- `plans/closure/search-eggsearch-integration/001-status.md`
- `plans/closure/search-eggsearch-integration/002-status.md`
- `plans/closure/search-eggsearch-integration/003-status.md`
- `plans/closure/search-eggsearch-integration/004-status.md`

Normative planning reference:

- `plans/003-planning-process.md#7-corrective-passes`

Current corrective implementation plan:

- `plans/implementation/search-eggsearch-integration/005-hosted-closure-sourcecard-fidelity-corrective-pass.md`

## 1. Corrective trigger

A post-M004 review on 2026-08-17 found that the M004 implementation corrected the principal deep-research consumer defect, but its strict closure disposition does not survive later exact-head evidence.

M004 implementation commit `6f1fa20af7a011c11ee905342694e1d58c46e94c` correctly moved deep research onto structured eggsearch dispatch, flattened `groups[*].results`, normalized workflow names, used structured security evidence, and retained structured `codesearch` values. Those implementation results remain historical evidence.

The later review found three narrower defects and one handoff-hygiene issue:

1. PR #78 exact head `77c10b98342777381f295aa1b16bb3d44999ae12` failed hosted CI run `31930352527`, job `95124064959`, in Workspace Clippy. The failure is directly in M004 code: `src/research/sources/eggsearch.rs` triggers `clippy::type_complexity` for the `result_items()` return type. Workspace tests were skipped after the lint failure.
2. M004 fixtures and conversion logic model provider/source-kind provenance using synthetic singular top-level fields such as `provider` and `source_type`. Current eggsearch 0.3.6 `SourceCard` instead uses `providers: Vec<String>` and nested `metadata.source_kind`, alongside `id`, `stable_id`, `title`, `url`, `snippet`, `trust`, `fetched`, `trust_markers`, and optional quality metadata.
3. M004 maps CodeGG `ResearchMode::LibraryEvaluation` to eggsearch `api_evaluation`. Eggsearch 0.3.6 exposes `library_comparison`, which is the semantically correct workflow for a library-evaluation/comparison request. The current mapping is accepted by upstream but loses workflow intent.
4. PR #78 remains draft with stale M001-only title/body/validation text even though the branch contains M001-M004 and now requires M005 closure. This does not change runtime correctness, but it obscures the actual review and closure state.

Under `plans/003-planning-process.md`, these findings require a new corrective milestone rather than rewriting M004's accepted record. M004 remains historical closure evidence; M005 controls the current strict disposition.

## 2. Why M004 verification missed the findings

The M004 local verification reportedly passed on the implementation environment, but the exact PR merge candidate later ran the repository's hosted `cargo clippy --workspace --all-targets --locked -- -D warnings` gate and exposed a lint failure not represented in the closure record.

The M004 research fixtures were current at the response-container level (`groups[*].results`) but not exact at the nested `SourceCard` field level. They therefore proved grouped-result traversal while allowing CodeGG to consume synthetic `provider` / `source_type` fields that real eggsearch cards do not normally emit.

The workflow test asserted that all emitted workflow strings were accepted upstream, but it did not assert semantic correspondence between each CodeGG mode and the most appropriate eggsearch workflow. As a result, `LibraryEvaluation -> api_evaluation` passed validation even though `library_comparison` is the correct upstream workflow.

## 3. Preserved invariants

M005 MUST preserve all accepted M001-M004 implementation invariants:

- eggsearch remains the default external search backend;
- `fallback_to_builtin` remains false by default;
- raw `mcp__eggsearch__*` tools remain hidden by default;
- `src/search/*` remains explicit compatibility fallback only;
- no direct Exa/Tavily/Brave/SerpAPI/Kagi execution path is reintroduced;
- CodeGG deep research continues to consume `dispatch_research_search_structured()` / `dispatch_security_search_structured()` values before display framing or truncation;
- current `groups[*].results` traversal remains stable and ordered;
- truncated display text is never authoritative when structured content is available;
- security review continues through `security_search` rather than a direct provider client;
- `codesearch` remains a compatibility alias over structured coding-profile `repo_search`;
- CodeGG's outer `external_untrusted` classification remains authoritative regardless of any upstream trust field;
- no storage migration, provider abstraction, MCP redesign, new background worker, new CI lane, scheduled compatibility job, version matrix, source scanner, release gate, or release automation is introduced.

## 4. Milestone 005 — Hosted closure and SourceCard fidelity corrective pass

Class: correctness / compatibility closure / polish

Objective:

Make the M004 implementation pass the repository's existing exact-head verification, consume real current eggsearch `SourceCard` provenance fields rather than fixture-only aliases, correct the library workflow mapping, and leave PR/registry state aligned with the actual completed search workstream.

Required deliverables:

- remove the `clippy::type_complexity` failure with a small maintainability-oriented type alias or helper decomposition; do not silence the lint unless a code-shape fix is demonstrably worse;
- update research conversion so real `SourceCard.providers` is retained deterministically in `SourceRecord` notes/provenance;
- read `metadata.source_kind` from current SourceCard JSON and use it for the existing CodeGG source-quality/source-kind projection where applicable;
- retain compatibility parsing for old/synthetic top-level aliases only if it remains small and does not obscure the canonical current shape;
- change `ResearchMode::LibraryEvaluation` to the upstream `library_comparison` workflow and preserve the other intentional mappings;
- replace or supplement M004 fixtures with at least one fixture shaped like a serialized eggsearch 0.3.6 `SourceCard`, including `providers`, `trust`, `fetched`, and nested `metadata.source_kind`;
- verify grouped research conversion, security conversion, truncation resistance, and `codesearch` structured retention still pass;
- run the existing hosted PR CI on the exact M005 candidate and require Workspace Clippy and Workspace tests to pass before strict closure;
- update PR #78 title/body so it describes the complete search/eggsearch integration work and current validation rather than only M001;
- create `plans/closure/search-eggsearch-integration/005-status.md` and return registry/addendum state to closed only after the exact candidate is green.

## 5. SourceCard contract baseline

The audited eggsearch package remains version `0.3.6`.

The canonical current `SourceCard` contract includes:

```text
SourceCard {
    id: String,
    stable_id: Option<String>,
    title: String,
    url: String,
    snippet: Option<String>,
    providers: Vec<String>,
    score: Option<f64>,
    trust: TrustLevel,
    fetched: bool,
    trust_markers: TrustMarkers,
    metadata: SourceMetadata,
    quality: Option<ResultQuality>,
}
```

`SourceMetadata.source_kind` is the canonical source-kind classification. M005 should consume that nested field first. Existing top-level `source_kind`, `source_type`, `type`, or `kind` compatibility may remain as secondary fallback only where it is useful for older/text-only responses.

Provider provenance should consume the `providers` array first and preserve its response order while avoiding duplicate entries. Do not infer provider credentials or provider configuration from these labels.

The upstream `trust` field is evidence metadata. It MUST NOT upgrade CodeGG's own trust classification; model-facing and research evidence remains `external_untrusted`.

## 6. Workflow mapping correction

M005 owns one semantic correction to the mapping established in M004:

| CodeGG mode | Required upstream workflow |
|---|---|
| `Landscape` | `ecosystem_survey` |
| `ArchitectureDecision` | `architecture_decision` |
| `LibraryEvaluation` | `library_comparison` |
| `ApiInvestigation` | `api_evaluation` |
| `DebuggingInvestigation` | `general` |
| `SecurityReview` | `security_review` via `security_search` |
| `SpecDigest` | `general` |
| `NarrowAnswer` | `general` |

Do not invent new eggsearch workflow values. If the upstream vocabulary changes during implementation, inspect the current accepted eggsearch contract and record the deviation in M005 closure evidence.

## 7. Verification policy

Verification remains deliberately minimal and directly tied to the discovered defects.

Required local/focused verification:

1. `cargo fmt --all -- --check`;
2. `git diff --check`;
3. `cargo clippy --workspace --all-targets --locked -- -D warnings` or the exact equivalent invoked by the repository's current CI;
4. `cargo test --lib research::sources::eggsearch -- --test-threads=1`;
5. `cargo test --test fake_eggsearch_mcp -- --test-threads=1`;
6. focused search-backend argument/structured-result tests if touched;
7. `scripts/verify.sh quick`.

Required hosted closure evidence:

- one ordinary existing PR `CI / verify` run on the exact final M005 head/merge candidate;
- Workspace Clippy green;
- Workspace tests green;
- no new workflow, matrix, scheduled job, or special compatibility lane added for M005.

The M003 real eggsearch 0.3.6 process smoke remains accepted wrapper-level evidence. M005 does not require another live-network compatibility matrix. A new real-process smoke is optional only if deterministic current-shaped fixtures leave a specific ambiguity.

## 8. Completion definition

This corrective addendum may return to `closed` only when all of the following are true:

- the exact final candidate no longer triggers the M004 `type_complexity` lint failure;
- current-shaped research fixtures use canonical `SourceCard.providers` and nested `metadata.source_kind`;
- provider/source-kind provenance from those fields survives into the existing CodeGG `SourceRecord` representation;
- `LibraryEvaluation` emits `library_comparison`;
- all other M004 structured-consumption, truncation, security, workflow, and `codesearch` invariants remain green;
- `scripts/verify.sh quick` passes;
- the existing hosted PR verification is green through Workspace tests on the exact accepted candidate;
- PR #78 metadata describes the complete workstream and its current validation state;
- `plans/closure/search-eggsearch-integration/005-status.md` records exact implementation commits, exact hosted run/job IDs, focused verification outcomes, compatibility evidence, and unresolved findings;
- registry state records M005 closed and removes it from dependency-ready work;
- no new search/provider ownership path or verification overengineering was introduced.

Until then, M001-M004 remain historical evidence, but the search/eggsearch subsystem's current strict disposition is controlled by M005.
