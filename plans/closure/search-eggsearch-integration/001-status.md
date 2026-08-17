# Search and Eggsearch Integration Milestone 001 — Closure Status

Status: closed

Source implementation plan: plans/implementation/search-eggsearch-integration/001-current-eggsearch-contract-repair.md

Source subsystem roadmap: plans/subsystems/search-eggsearch-integration-roadmap.md#milestone-001--current-eggsearch-request-contract-repair

Repository baseline reviewed: acb6ba8092cee2abe6b0bf3ab61960968aab36a9

Implementation commits: acb6ba8092cee2abe6b0bf3ab61960968aab36a9 — Repair eggsearch request contracts

Eggsearch contract baseline: 0.3.6, audited release commit 4ccb374af00348bba75761f6bbd1e192d385a2b9

## 1. Executive finding

M001 is strictly closed. CodeGG now normalizes every advertised eggsearch wrapper request to the current 0.3.6 MCP contract, preserves only unambiguous compatibility aliases, rejects unsupported legacy semantics before MCP invocation, and uses strict offline fake-server validation so stale request shapes fail tests.

No live eggsearch process or network compatibility smoke was claimed; that evidence remains owned by M003.

## 2. Requirement-to-evidence matrix

| CodeGG wrapper | Current upstream request contract | Compatibility disposition | Evidence |
|---|---|---|---|
| websearch | query, max_results, optional providers, intent, freshness, safe_search | Existing num_results and provider hint aliases retained | Mapping and fake MCP tests |
| webfetch | url, max_chars, extract_mode, include_links | Existing max_length alias retained; options now pass through | Mapping and fake MCP tests |
| repo_search | Structured repo hints with owner/repo normalization, current profile/mode/local fields | include_snippets rejected; combined locator split only when unambiguous | Fake MCP exact request assertions |
| repo_fetch | Separate owner, repo, path, line_start, line_end plus current optional fields | start_line/end_line translate with conflict rejection; combined locator retained | Fake MCP exact request assertions and range unit tests |
| repo_map | Separate owner, repo, max_depth and current map options | depth translates; non-empty historical path rejects clearly | Fake MCP assertions and negative integration test |
| security_search | Current identifier, package/version, applicability, provider, and workflow fields | cve translates to cve_id; current GHSA/OSV/RustSec fields exposed | Fake MCP exact request assertions |
| research_search | Current research domain/source type/workflow/depth/provider fields | domains translates only for a clear provider list or one domain; ambiguity rejects | Fake MCP assertions and unit tests |
| batch_fetch | Non-empty tagged items containing web or structured repo items | Legacy urls, combined repo locators, and range aliases normalize | Strict fake validation, mixed-batch and empty-batch tests |
| evidence_bundle | Current source-card sources and linked fetches | Historical pseudo-source type rejects rather than being dropped; fetch-only input is supported | Current-input and legacy-rejection tests |

provider_status remains a diagnostic call through the shared MCP service and was not given a stale request shape.

## 3. Production implementation evidence

- Added one shared repository locator normalizer supporting explicit fields, unambiguous combined locators, host/ref forwarding, and actionable malformed/ambiguous errors.
- Added shared line-range alias handling and batch/evidence request builders in src/search_backend/eggsearch.rs.
- Updated all nine model-facing wrapper schemas to describe current fields and retained aliases.
- Preserved the shared MCP service, timeout, output cap, trust framing, default eggsearch backend, hidden raw MCP tools, and disabled built-in fallback defaults.
- Added strict per-tool fake MCP validation covering required fields and stale names.
- Added current contract documentation to architecture/search_backend.md and architecture/tool.md.

## 4. Verification executed

All commands were run locally against the accepted revision:

- cargo fmt --all -- --check — passed via scripts/verify.sh quick.
- git diff --check — passed.
- cargo test --test search_backend_arg_mapping -- --test-threads=1 — passed.
- cargo test --test search_backend_eggsearch -- --test-threads=1 — passed.
- cargo test --test fake_eggsearch_mcp -- --test-threads=1 — passed.
- cargo test --lib — passed.
- cargo clippy --all-targets -- -D warnings — passed.
- scripts/verify.sh quick — passed, including workspace all-target checks and repository static guards.

No network-required or real-binary test was run.

## 5. Invariant review

- Default backend remains eggsearch.
- Raw mcp__eggsearch__* tools remain hidden by default.
- fallback_to_builtin remains false by default and was not expanded.
- External outputs remain external_untrusted, bounded, and framed.
- The shared MCP service remains the execution owner; no per-call process or provider client was added.
- No CodeGG-owned provider routing stack, persistent search state, database migration, CI lane, compatibility matrix, or release automation was added.

## 6. Failure and recovery review

Validation failures are pure, deterministic, and occur before MCP invocation. MCP transport errors, timeouts, cancellation, shared-service lifecycle, and restart semantics remain on the existing paths. The new strict fake-server tests prove local validation failures do not invoke MCP.

## 7. Migration and compatibility review

No durable migration is required. Retained aliases are repo = owner/name, start_line/end_line, cve, and top-level urls; each has a one-to-one translation. repo_map.path, ambiguous research domains, stale include_snippets, and historical evidence pseudo-source type are rejected with actionable errors.

## 8. Security review

URL validation remains in place for web and batch fetches. Repository normalization does not access the filesystem. Credential values are not copied into errors or model-visible output. External content remains data rather than instruction-trusted content.

## 9. Documentation and operations

Updated architecture/search_backend.md and architecture/tool.md. No installation guidance or config defaults required correction. M003 remains responsible for real-current-eggsearch compatibility evidence and capability diagnostics.

## 10. Unresolved findings

None in M001 scope. Real-process compatibility is intentionally deferred to M003 and is not a closure defect for this milestone.

## 11. Roadmap disposition

M001 is closed. M002 is dependency-ready because its sole hard dependency is this accepted closure and its interface contract is stable. M003 remains blocked on M002 closure and requires a locally runnable current eggsearch binary for its final operational evidence.

## 12. Registry updates

- M001 moved from active implementation to recently closed.
- M002 moved from blocked to ready in the same closure change.
- M003 remains blocked on M002 and its separately named operational evidence.
- No corrective pass was created; no high, medium, or low unresolved M001 finding remains.
