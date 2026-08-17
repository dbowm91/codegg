# Search and Eggsearch Integration M005 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/search-eggsearch-integration/005-hosted-closure-sourcecard-fidelity-corrective-pass.md`

Source corrective addendum: `plans/subsystems/search-eggsearch-integration-hosted-closure-sourcecard-fidelity-corrective-addendum.md`

Source subsystem roadmap: `plans/subsystems/search-eggsearch-integration-roadmap.md`

Historical predecessor evidence:

- `plans/closure/search-eggsearch-integration/001-status.md`
- `plans/closure/search-eggsearch-integration/002-status.md`
- `plans/closure/search-eggsearch-integration/003-status.md`
- `plans/closure/search-eggsearch-integration/004-status.md`

Implementation commit: `75ccc70e66f49b80e6daca7fc08cbe1eedcbd301` — `fix(search): align eggsearch source card fidelity`

Final accepted candidate: `75ccc70e66f49b80e6daca7fc08cbe1eedcbd301`

Exact hosted evidence: PR #78, CI run `32047863303`, verify job `95439829669` — green through Workspace Clippy and Workspace tests.

## 1. Executive finding

M005 is strictly closed. The corrective pass removes the hosted M004
`clippy::type_complexity` failure with a local result-item alias, consumes
canonical eggsearch 0.3.6 SourceCard provenance (`providers` and nested
`metadata.source_kind`) before compatibility aliases, maps
`LibraryEvaluation` to `library_comparison`, and preserves all accepted M004
structured-consumption, truncation, security, network-budget, and codesearch
behavior.

The exact final production candidate passed the ordinary PR verification run
through Workspace tests. M004 remains historical implementation evidence; its
later exact-head failure and fidelity findings are preserved rather than
rewritten.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence |
|---|---|
| Hosted Clippy defect cleared | `ResultItem<'a>` removes the complex inline return type; local exact Clippy and hosted run `32047863303` / job `95439829669` passed. |
| Canonical provider fidelity | `providers` arrays are trimmed, deduplicated in response order, and emitted as deterministic `provider=...` notes; source unit and fake-MCP tests assert multiple providers. |
| Canonical source-kind fidelity | `metadata.source_kind` is primary, followed by bounded legacy aliases; official docs and advisory fixtures assert the existing quality/provenance projection. |
| Trust remains CodeGG policy | Current-shaped fixtures include `trust`, `fetched`, and `trust_markers`; converted records retain `trust=external_untrusted` and never elevate upstream trust. |
| Unknown additive fields | Current-shaped fixtures include unknown fields and conversion succeeds without schema equality or failure. |
| Library workflow semantics | Exhaustive mode mapping asserts `LibraryEvaluation -> library_comparison` and `ApiInvestigation -> api_evaluation`, with all other mappings covered. |
| Structured value authority | Existing `convert_structured` path continues to prefer `StructuredSearchResult.value` over bounded display output. |
| Truncation resistance | Structured-value and text-only-truncated regression tests remain green; fake-MCP research uses a tiny display cap while retaining all three structured cards. |
| Security path | Security review continues through `security_search`, converts a current-shaped advisory card, and preserves provider/source-kind provenance. |
| Network budget | Network-disabled research/security requests fail before MCP dispatch. |
| Codesearch compatibility | Existing structured repo-search value/profile retention test remains green. |
| Ownership/security boundary | No direct provider client, credential path, new MCP boundary, or trust elevation was introduced. |

## 3. Production implementation evidence

- `src/research/sources/eggsearch.rs` keeps grouped `groups[*].results` traversal
  ordered and filters non-HTTP(S) URLs.
- Canonical `providers` arrays are the primary provider source. Empty values are
  ignored and duplicate labels are removed without reordering. Singular item or
  group aliases remain only as bounded compatibility fallbacks.
- Canonical nested `metadata.source_kind` is the primary source-kind source.
  Existing `SourceQuality` values are reused; no persistent taxonomy or storage
  schema was added.
- SourceRecord notes retain stable identity, provider labels, source kind, group
  label, snippets, and CodeGG's `external_untrusted` classification. No
  credentials, routing configuration, provider failures, or control instructions
  enter the notes.
- `src/tool/codesearch.rs` was not functionally changed; its accepted structured
  behavior remains intact.

## 4. Verification executed

Local verification:

- `cargo fmt --all -- --check` — passed.
- `git diff --check` — passed.
- `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --locked -- -D warnings` — passed with no issues.
- `cargo test --lib research::sources::eggsearch -- --test-threads=1` — 5 passed.
- `cargo test --test fake_eggsearch_mcp -- --test-threads=1` — 28 passed.
- `cargo test --test search_backend_eggsearch -- --test-threads=1` — 9 passed.
- `cargo test --test search_backend_arg_mapping -- --test-threads=1` — 11 passed.
- `cargo test --test search_backend_legacy -- --test-threads=1` — 4 passed.
- `CARGO_BUILD_JOBS=1 scripts/verify.sh quick` — passed, including generated-agent, boundary, sandbox, execution-ownership, formatting, and locked workspace checks.

Hosted verification on the exact accepted production candidate:

- PR #78 CI run `32047863303` — passed.
- Verify job `95439829669` — passed in 22m59s.
- Candidate SHA `75ccc70e66f49b80e6daca7fc08cbe1eedcbd301` — exact head.
- Workspace Clippy — passed.
- Workspace tests — executed and passed with the repository's serialized test command.
- No CI workflow, matrix, scheduled job, compatibility lane, source scanner,
  coverage gate, or release gate was added.

The accepted M003 real eggsearch 0.3.6 process smoke remains historical wrapper
compatibility evidence. M005 needed no new network-dependent compatibility
campaign because current-shaped deterministic consumer fixtures and the exact
hosted gate close the discovered boundaries.

## 5. Invariant review

- Eggsearch remains the sole normal external search/provider owner.
- Structured values remain authoritative over model-facing display framing.
- Truncated text-only output still fails explicitly rather than being parsed as
  partial JSON.
- `codesearch` remains a thin coding-profile `repo_search` compatibility alias.
- Existing persisted `SourceRecord` data remains readable; no migration was
  required.
- Unknown additive upstream fields remain safely ignored.
- CodeGG's own `external_untrusted` classification remains authoritative.

## 6. Failure and recovery review

Network-disabled collection still exits before backend dispatch. Invalid and
non-HTTP(S) source cards are discarded without losing valid siblings. Missing
structured values with truncated display output return a bounded source-
collection error. No asynchronous worker, retry authority, process lifecycle,
or recovery behavior changed in M005.

## 7. Migration and compatibility review

No storage, protocol, or dependency migration was needed. The converter accepts
the current eggsearch 0.3.6 SourceCard fields without requiring an exact JSON
shape and retains narrow legacy aliases for older responses. Existing source
quality and notes representations remain compatible.

## 8. Security review

Provider labels are recorded only as provenance. They do not select providers,
read credentials, or alter routing. Upstream trust labels, fetched state, trust
markers, snippets, labels, and unknown metadata remain evidence/data only.
External URLs remain restricted to HTTP(S), and no `next_actions` or advisory
content is executed as CodeGG control input.

## 9. Documentation and operations

The implementation plan is marked `implemented`, the corrective addendum and
search subsystem registry state are closed, and this record captures the exact
hosted evidence. PR #78 metadata is reconciled to the complete M001–M005
workstream and current validation. The PR remains draft for review.

## 10. Unresolved findings

None at critical, high, medium, low, or informational severity within M005
scope. The explicit legacy generic fallback and accepted M003 real-process smoke
remain documented compatibility/history items, not open corrective findings.

## 11. Roadmap disposition

M005 closes the hosted-closure and SourceCard-fidelity corrective boundary. The
search/eggsearch integration roadmap and its corrective addenda may remain
closed; no additional search milestone is required by the evidence collected in
this pass.

## 12. Registry updates

- M005 moved from active implementation to closed and was removed from the
  dependency-ready section.
- The corrective addendum and search/eggsearch subsystem were returned to
  `closed`.
- The blocked-work section and affected subsystem dependency graphs were
  audited. No registered plan listed M005 as its remaining hard or interface
  blocker, so no downstream plan became ready. Tool Programs M019 remains ready;
  Provider M007 remains conditionally closed; Development Verification and
  Release M006 remains blocked on those independent closure records.
- M004 was not rewritten; its historical record remains intact and explicitly
  records that later evidence transferred current strict closure to M005.

Recommendation: `closed`.
