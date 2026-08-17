# Search and Eggsearch Integration Milestone 002 — Closure Status

Status: closed

Source implementation plan: plans/implementation/search-eggsearch-integration/002-external-search-ownership-consolidation.md

Source subsystem roadmap: plans/subsystems/search-eggsearch-integration-roadmap.md#milestone-002--external-search-ownership-consolidation

Repository baseline reviewed: e46f97d2

Implementation commits: e46f97d2 — Consolidate external search ownership in eggsearch

Accepted predecessor closure: plans/closure/search-eggsearch-integration/001-status.md

## 1. Executive finding

M002 is strictly closed. CodeGG now has one normal external search owner:
the shared eggsearch MCP/search-backend boundary. The model-facing
`codesearch` name is retained as a compatibility alias over eggsearch
`repo_search` with `profile = "coding"`; it no longer constructs an Exa
request, reads an Exa key, or owns an HTTP client. Deep research now uses a
single eggsearch-backed source adapter, with security reviews selecting the
eggsearch security-search class, and preserves CodeGG ownership of local
collection, budgeting, synthesis, persistence, and report generation.

The legacy `src/search/*` provider stack remains only for the explicitly
configured generic `backend = "builtin"` compatibility path (or its
explicit generic fallback setting). It is not used by the default eggsearch
backend, by `codesearch`, or by research external-source collection.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence |
|---|---|
| M001 accepted before M002 | `plans/closure/search-eggsearch-integration/001-status.md` is closed; current request mappings remained green in focused suites |
| No normal direct Exa `codesearch` path | `src/tool/codesearch.rs` only validates/normalizes input and calls `dispatch_repo_search`; fake MCP test records `repo_search` with `profile = "coding"` |
| No Exa credential at the codesearch boundary | Exa environment reads and direct `api.exa.ai/code` request construction were deleted |
| Research external collection uses eggsearch | `EggsearchSource` is registered by `ResearchCoordinator::new`; it calls shared `dispatch_research_search` or `dispatch_security_search` and converts result cards into `SourceRecord` |
| Network budget remains fail-closed | `EggsearchSource::collect` returns `NetworkNotAllowed` before dispatch; fake MCP test observes zero calls when `allow_network = false` |
| External source provenance survives conversion | Converted records carry `source=eggsearch`, `trust=external_untrusted`, provider notes when available, URL/title, publication date, and quality mapping |
| Local/synthesis responsibilities remain | Existing local, URL, crate, GitHub, docs, claim, extraction, storage, and synthesis paths remain; only the duplicate provider collector was replaced |
| Legacy backend remains explicit | `SearchConfig`/`search_backend::legacy` behavior and `search_backend_legacy` tests remain unchanged and green; specialized wrappers still reject builtin mode |
| Registry/prompts/docs reflect ownership | `codesearch` description, agent availability logic, prompt contract, search/research/tool/security/native-crate architecture docs, and plan controls were updated |

## 3. Production implementation evidence

### Before inventory

- `src/tool/codesearch.rs` directly read `EXA_API_KEY`/`EXA_CODE_API_KEY`, built a
  `reqwest::Client`, validated `api.exa.ai`, and posted to the Exa Code API.
- `src/research/sources/search_provider.rs` directly implemented Tavily,
  Brave, SerpAPI, and Kagi clients, including provider-specific URLs,
  credentials, DTOs, and result conversion.
- The normal research coordinator did not have an eggsearch external source;
  the compatibility constructor selected the direct provider source.
- `src/search/*` was already the legacy generic web-search fallback.

### After inventory

- `codesearch` maps its bounded query and legacy token budget to current
  eggsearch `repo_search` fields and receives normal eggsearch framing and
  provenance.
- `EggsearchSource` is part of the default coordinator adapter set. It uses
  `research_search` for broad research and `security_search` for
  `SecurityReview`, never selecting a provider enum or reading a provider
  key. The old provider constructor remains parseable only as a deprecated,
  inert compatibility shim whose arguments are ignored.
- Research result conversion is intentionally narrow: URL/title/result
  metadata is mapped into existing `SourceRecord` fields and provider/trust
  context is retained in notes. No provider-specific research DTOs remain.
- The shared process-wide MCP service remains the execution owner; no
  per-research-run eggsearch process or client was added.
- `src/search/*` remains a documented, explicit generic compatibility branch.
  No specialized eggsearch tool falls through to it.

## 4. Verification executed

All commands were run locally against the implementation revision unless
otherwise noted:

- `cargo fmt --all` — passed.
- `git diff --check` and staged `git diff --cached --check` — passed.
- `cargo check --all-targets --locked` — passed.
- `cargo test --test fake_eggsearch_mcp -- --test-threads=1` — passed, 25 tests.
- `cargo test --test search_backend_eggsearch -- --test-threads=1` — passed, 9 tests.
- `cargo test --test search_backend_legacy -- --test-threads=1` — passed, 4 tests.
- `scripts/verify.sh quick` — passed, including generated-agent, boundary,
  ownership, formatting, and workspace all-target checks.
- `cargo test --lib` — one existing timing-sensitive daemon-socket test
  failed once (`socket_f6_typed_peer_error_recovery_converges_25_cycles`);
  the exact test rerun with `--test-threads=1` passed. No failure involved
  the changed search/research code.

Targeted final-tree inspection:

- No `api.exa.ai/code`, `EXA_CODE_API_KEY`, or direct Exa code execution
  remains under `src/` or `tests/`.
- No Tavily, Brave Search, SerpAPI, or Kagi endpoint remains in the
  executable research collection path; `src/research/sources/eggsearch.rs`
  contains no provider endpoint or credential handling.
- The remaining endpoint strings are confined to `src/search/providers.rs`,
  which is the intentionally retained generic legacy provider implementation
  reachable only through explicit `backend = "builtin"` compatibility or
  explicit generic fallback. This is classified compatibility behavior, not
  a normal/default or research execution bypass.

## 5. Invariant review

- Eggsearch remains the default backend and raw eggsearch MCP tools remain
  hidden by default.
- `fallback_to_builtin` remains false by default.
- External output remains trust-framed as untrusted evidence.
- Network-disabled research performs no eggsearch call.
- Shared MCP lifecycle, timeout, cancellation, and process ownership remain
  unchanged.
- No new provider abstraction, search index, cache, database migration, CI
  lane, scanner, or release automation was introduced.

## 6. Failure and recovery review

Eggsearch-unavailable and specialized-backend failures surface through the
existing bounded source-collection warning/error path; there is no direct
provider fallback. Request parsing and source conversion failures are
actionable `SourceCollection` errors. Network-disabled collection exits
before backend lookup. No new process lifecycle or mutable per-run shared
state was introduced.

## 7. Migration and compatibility review

No durable migration is required, and historical research artifacts remain
readable. The `codesearch` name and `tokens_num` input remain available; its
budget is translated to bounded `max_results` rather than promising Exa-only
token semantics. The old `SearchProvider`/`with_search_provider` surface is
deprecated and inert: old provider values do not reactivate direct clients
or credentials. Explicit generic builtin search configuration remains
parseable and tested.

## 8. Security review

The direct Exa/Tavily/Brave/SerpAPI/Kagi credential and HTTP handling was
removed from the normal CodeGG paths. External result URLs remain data and
are converted only when they are valid HTTP(S) URLs. Converted records carry
`external_untrusted` provenance notes; this is evidence provenance, not
instruction trust. Existing SSRF and untrusted-HTTP controls remain for
CodeGG-owned explicit URL/builtin compatibility paths.

## 9. Documentation and operations

Updated `architecture/search_backend.md`, `architecture/research.md`,
`architecture/tool.md`, `architecture/security.md`,
`architecture/native_crates.md`, and the agent web-search contract. Updated
the implementation plan, subsystem roadmap, and registry lifecycle entries.

No direct provider credential instructions were added. M003 remains the
owner of structured result preservation, capability diagnostics, and the
bounded real eggsearch process compatibility smoke.

## 10. Unresolved findings

None at critical, high, medium, or low severity within M002 scope. The
legacy builtin provider endpoint strings are an intentional documented
compatibility path and are not an unresolved ownership defect.

## 11. Roadmap disposition

M002 is closed. The subsystem roadmap remains active because M003 is not yet
closed; it is now dependency-ready. M003 retains its operational requirement
for a locally runnable current eggsearch binary at final closure, but that
requirement no longer blocks implementation handoff.

## 12. Registry updates

- M002 moved from active implementation to recently closed with this record.
- M003's hard M002 blocker is resolved; it moved from blocked to ready in the
  same closure change.
- M003's separate operational real-binary evidence requirement remains
  explicitly recorded as a final-closure condition.
- No corrective pass was created; no unresolved M002 finding remains.
