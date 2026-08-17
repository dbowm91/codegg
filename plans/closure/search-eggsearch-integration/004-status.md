# Search and Eggsearch Integration M004 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/search-eggsearch-integration/004-deep-research-structured-consumption-corrective-pass.md`

Source corrective addendum: `plans/subsystems/search-eggsearch-integration-deep-research-corrective-addendum.md`

Source subsystem roadmap: `plans/subsystems/search-eggsearch-integration-roadmap.md`

Accepted historical predecessors:

- `plans/closure/search-eggsearch-integration/001-status.md`
- `plans/closure/search-eggsearch-integration/002-status.md`
- `plans/closure/search-eggsearch-integration/003-status.md`

Implementation commit: `6f1fa20a` — `fix(search): consume structured eggsearch research results`

## 1. Executive finding

M004 is strictly closed. The ordinary CodeGG deep-research source collector now
consumes the complete structured eggsearch value before display framing or
truncation, flattens current `groups[*].results` source cards in stable order,
maps every CodeGG `ResearchMode` to a documented eggsearch workflow, converts
structured security evidence through the same boundary, and retains structured
repo-search metadata through the `codesearch` compatibility alias.

M001–M003 remain historical accepted evidence. This record controls the current
strict disposition of the corrective addendum and search/eggsearch subsystem; it
does not rewrite M003's accepted closure record.

## 2. Requirement-to-evidence matrix

| Acceptance criterion | Evidence |
|---|---|
| Grouped research cards produce sources | `EggsearchSource::result_items()` consumes every `groups[*].results[*]`; unit fixture converts three valid cards across two groups. |
| Stable order and invalid URL filtering | Unit test preserves response order and rejects the `file://` sibling without losing valid cards. |
| Structured value is authoritative | `collect_external()` calls `dispatch_research_search_structured()` / `dispatch_security_search_structured()` and `convert_structured()` uses `value` before any display parse. |
| Display truncation does not lose evidence | Fake-MCP research test sets `max_research_output_chars = 40` and still receives all three structured source cards; unit test uses a conflicting truncated projection. |
| Text-only truncation fails explicitly | Unit test verifies a truncated result with `value = None` returns a bounded `SourceCollection` error. |
| All research modes are mapped | Exhaustive unit test covers all eight `ResearchMode` variants: `ecosystem_survey`, `architecture_decision`, `api_evaluation`, `api_evaluation`, `general`, `security_review`, `general`, `general`. |
| Only supported workflows cross MCP | Fake-MCP capture asserts `Landscape -> ecosystem_survey`; no CodeGG enum names are serialized. |
| Security uses structured security evidence | Fake-MCP `SecurityReview` test calls only `security_search`, captures `security_review`, and converts the advisory source card. |
| Network budget remains fail-closed | Research and security integration tests assert `allow_network = false` returns `NetworkNotAllowed` with no additional MCP call. |
| `codesearch` retains structured repo metadata | `codesearch_structured_execution_retains_repo_search_value` asserts `value.stable_id = repo-1`, `profile = coding`, and bounded framed output. |
| No direct provider client reintroduced | Final-tree inspection found no new provider client, credential read, or endpoint; legacy provider strings remain only in explicit compatibility code. |
| Default/fallback/raw-tool/trust invariants remain | Existing fake-MCP and legacy backend suites remain green; structured output remains externally framed and raw tools remain hidden by default. |
| Focused tests green | 5 source unit tests, 28 `fake_eggsearch_mcp` tests, 9 `search_backend_eggsearch` tests, 4 `search_backend_legacy` tests, and 11 argument-mapping tests passed. |
| Formatting and diff checks green | `cargo fmt --all -- --check` and `git diff --check` passed. |
| Quick verification green | `scripts/verify.sh quick` passed, including generated-agent, boundary, sandbox, execution-ownership, and locked all-target checks. |
| Documentation and closure evidence complete | `architecture/research.md`, `architecture/search_backend.md`, `architecture/tool.md`, this record, the addendum, roadmap, implementation plan, and registry were updated. |

## 3. Production implementation evidence

- `EggsearchSource` now uses the structured dispatch surface for both normal
  research and security review. The string projection remains only a degraded
  compatibility path when no structured value exists and the projection is not
  truncated.
- Group metadata, provider, source kind, stable/source IDs, publication data,
  title, snippet, trust, and eggsearch identity are retained in the existing
  `SourceRecord` fields and notes. No storage migration was needed.
- The `codesearch` input normalization is shared by legacy and structured
  execution, while structured execution calls `dispatch_repo_search_structured`
  directly and uses the existing provenance/value helper.
- No new process, cache, provider abstraction, protocol, scheduler, or CI lane
  was introduced.

## 4. Current response shapes consumed

The research and security consumer accepts the current grouped response shape:

```text
{
  "groups": [
    {
      "classification": "...",
      "label": "...",
      "results": [
        { "url": "https://...", "title": "...", ... }
      ]
    }
  ]
}
```

The converter also retains the narrow legacy top-level array compatibility for
older/text-only responses. Unknown additive fields are ignored safely; no
upstream `next_actions` or advisory guidance is executed as control input.

## 5. Workflow mapping evidence

| CodeGG mode | Upstream tool | Workflow |
|---|---|---|
| `Landscape` | `research_search` | `ecosystem_survey` |
| `ArchitectureDecision` | `research_search` | `architecture_decision` |
| `LibraryEvaluation` | `research_search` | `api_evaluation` |
| `ApiInvestigation` | `research_search` | `api_evaluation` |
| `DebuggingInvestigation` | `research_search` | `general` |
| `SecurityReview` | `security_search` | `security_review` |
| `SpecDigest` | `research_search` | `general` |
| `NarrowAnswer` | `research_search` | `general` |

## 6. Verification executed

- `cargo fmt --all -- --check` — passed.
- `git diff --check` — passed.
- `cargo test --lib research::sources::eggsearch -- --test-threads=1` — 5 passed.
- `cargo test --test fake_eggsearch_mcp -- --test-threads=1` — 28 passed.
- `cargo test --test search_backend_eggsearch -- --test-threads=1` — 9 passed.
- `cargo test --test search_backend_legacy -- --test-threads=1` — 4 passed.
- `cargo test --test search_backend_arg_mapping -- --test-threads=1` — 11 passed.
- `scripts/verify.sh quick` — passed.

The existing M003 local real-process eggsearch 0.3.6 smoke remains accepted
historical evidence. M004 did not add a network-dependent test or repeat that
wrapper-level smoke because the deterministic consumer-path evidence is
complete.

## 7. Security, compatibility, and failure review

- External source URLs are accepted only for HTTP(S) schemes.
- Source-card text, snippets, advisory metadata, trust markers, and
  `next_actions` remain evidence/data and are never interpreted as CodeGG
  instructions.
- Provider credentials remain owned by eggsearch and are not copied into
  `SourceRecord` notes or errors.
- Existing persisted `SourceRecord` artifacts remain readable; no migration is
  required.
- Network-disabled research exits before backend dispatch.
- Shared MCP ownership, timeout, cancellation, bounded model output, hidden raw
  tools, default eggsearch selection, and explicit legacy fallback semantics are
  unchanged.

## 8. Unresolved findings

None at critical, high, medium, low, or informational severity within M004
scope. The explicit legacy generic provider fallback and the accepted M003
real-process smoke remain documented compatibility/history items, not current
corrective findings.

## 9. Roadmap and dependency disposition

The corrective addendum and search/eggsearch roadmap return to `closed`. M004 is
removed from dependency-ready work and is recorded as the controlling corrective
closure point.

The registry dependency audit found no other registered plan whose blocker is
resolved by M004. Tool Programs M019 remains ready; Provider M007 remains
conditionally closed; Development Verification and Release M006 remains blocked
on those independent Provider M007 and Tool Programs M019 closure records. No
status was changed outside the search/eggsearch workstream.

Recommendation: `closed`.
