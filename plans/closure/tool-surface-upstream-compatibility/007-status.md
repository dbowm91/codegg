# Tool-Surface Upstream Compatibility M007 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-surface-upstream-compatibility/007-eggsearch-0.3.9-surface-alignment.md`

Source subsystem roadmap:

- `plans/subsystems/tool-surface-upstream-compatibility-corrective-addendum.md#milestones`

Repository baseline reviewed: `2798501b61edcd43acb215924500635dc705bbb2`

Implementation commits:

- `0cd35ab` — align CodeGG eggsearch wrappers, structured result status, schemas, and fixtures with 0.3.9;
- `a86e6fa` — move the implementation plan into closure review;
- this closure commit — accept evidence, close M007, and reconcile the dependency registry.

Audited upstream contract: eggsearch `0.3.9`, tag `v0.3.9` at `0cbbeee`.

## 1. Executive finding

M007 is fully implemented and closed. Every CodeGG-wrapped eggsearch tool was
compared with the audited 0.3.9 contract. The stable facade now forwards the
provider-neutral search excerpt control, focused fetch controls, tightening
cache controls, and batch web-item controls with strict validation. CodeGG
requests diagnostic detail internally, retains structured content and tool
failure status separately from bounded model text, and continues to keep raw
MCP tools hidden and next actions data-only.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Audit every wrapped tool | 0.3.9 source/schema comparison recorded below; adapter builders in `src/search_backend/eggsearch.rs` | pass | Covers web, repo, security, research, batch, evidence, and provider-status surfaces |
| Search `excerpt_count` | `tests/search_backend_arg_mapping.rs::excerpt_count_and_internal_response_detail_reach_upstream` | pass | Explicit `0` and `3` survive; `4` is rejected |
| Fetch `focus` and bounds | `fetch_focus_and_cache_controls_reach_upstream`; native webfetch schema | pass | Non-empty, max 512 characters, chunk/character bounds, metadata-only conflict rejected |
| Fetch cache controls | Mapping/rejection tests and fake-MCP range checks | pass | `default`/`bypass`/`refresh`; max age capped at 2,592,000 seconds; bypass+age conflict rejected |
| Batch web-item controls | `batch_fetch_normalizes_mixed_legacy_repo_and_web_items` | pass | Focus and cache controls are retained during normalization |
| Explicit response-detail policy | Adapter forces `response_detail=diagnostic`; architecture policy section | pass | Not exposed as a native user option |
| Full structured evidence and failure state | `structured_wrappers_preserve_upstream_value_and_bound_display`; `StructuredSearchResult.success` | pass | Warnings, provider/retrieval state, trust markers, IDs, next actions, unknown fields, and MCP `isError` status survive |
| Compatibility fixture hardening | `validate_current_eggsearch_request` in `tests/fake_eggsearch_mcp.rs` | pass | Current names, enums, ranges, and response policy are checked at the fixture boundary |
| Modern MCP metadata use | M006 `McpService::call_tool_structured` seam reused | pass | No eggsearch-specific protocol plumbing or raw metadata exposure added |
| No provider HTTP/cache regression | Diff and ownership review of adapter | pass | Eggsearch remains the external search owner; CodeGG adds no cache layer |

### Concise 0.3.9 wrapper compatibility matrix

| Wrapper | Represented/current translation | Internal/defaulted | Intentionally deferred or rejected |
|---|---|---|---|
| `web_search` | query, result limit, provider hint, intent, freshness, safe search, `excerpt_count` | timeout/defaults, diagnostic detail | provider-specific fields, date/domain/language/region knobs |
| `web_fetch` | URL, max chars, extract mode, links, `focus`, focus bounds, cache policy, max cache age | timeout/defaults, diagnostic detail | PDF/render/browser-profile controls |
| `repo_search` | repository query/locator, provider and result-limit compatibility fields | diagnostic detail | provider-specific ranking and transport fields |
| `repo_fetch` | locator, path, line aliases/ranges, context and output limits | diagnostic detail | provider-specific and low-value extraction fields |
| `repo_map` | locator and bounded depth/entry mapping | diagnostic detail | provider-specific repository traversal fields |
| `security_search` | query, package/security filters, provider and result-limit compatibility fields | diagnostic detail | provider-specific advisory/routing fields |
| `research_search` | query/domain/provider translation, result limits, workflow compatibility fields | diagnostic detail | provider-specific research controls |
| `batch_fetch` | tagged web/repo items, limits, continuation; web focus/cache controls | diagnostic detail | unsupported cache controls on repo items |
| `build_evidence_bundle` | goal, sources, fetches, inclusion and size limits | diagnostic detail is forced at call boundary | upstream provider-specific controls |
| `provider_status` | diagnostic helper path and provider-status arguments | diagnostic-only upstream surface | no response-detail parameter; no raw diagnostic metadata projection |

Legacy CodeGG aliases with unambiguous semantics remain translated. Stale,
ambiguous, conflicting, or out-of-contract arguments fail before MCP dispatch.

## 3. Production implementation evidence

- `src/search_backend/eggsearch.rs` centralizes 0.3.9 bounds and validates
  integer, enum, focus, and cache combinations before constructing upstream
  arguments. It applies the same internal diagnostic policy to all structured
  eggsearch wrappers.
- Native schemas in `src/tool/websearch.rs`, `src/tool/webfetch.rs`, and
  `src/tool/batch_fetch.rs` expose only the selected provider-neutral controls.
  They do not expose upstream `response_detail` or provider-specific knobs.
- `src/search_backend/mod.rs` carries the upstream tool-level success state
  through `StructuredSearchResult` into `StructuredToolResult`; full values
  remain separate from capped/framed display output.
- Fake MCP fixtures validate the live field names, enums, ranges, and
  response-detail policy. The structured fixture includes partial retrieval,
  provider failure, trust, warning, stable-ID, suggested-action, and unknown
  additive metadata.
- No direct provider client, persistent CodeGG cache, raw-MCP exposure, or
  automatic next-action execution was introduced.

## 4. Verification executed

### Commands run

```text
rtk cargo check --test search_backend_arg_mapping --test fake_eggsearch_mcp --test search_backend_eggsearch
rtk proxy env LZMA_API_STATIC=1 RUSTFLAGS='-C link-arg=-L/usr/local/opt/xz/lib' CARGO_BUILD_JOBS=1 cargo test --test search_backend_arg_mapping -- --test-threads=1
rtk proxy env LZMA_API_STATIC=1 RUSTFLAGS='-C link-arg=-L/usr/local/opt/xz/lib' CARGO_BUILD_JOBS=1 cargo test --test fake_eggsearch_mcp -- --test-threads=1
rtk proxy env LZMA_API_STATIC=1 RUSTFLAGS='-C link-arg=-L/usr/local/opt/xz/lib' CARGO_BUILD_JOBS=1 cargo test --test search_backend_eggsearch -- --test-threads=1
rtk proxy env LZMA_API_STATIC=1 RUSTFLAGS='-C link-arg=-L/usr/local/opt/xz/lib' CARGO_BUILD_JOBS=1 cargo test --lib research::sources::eggsearch -- --test-threads=1
rtk proxy env LZMA_API_STATIC=1 RUSTFLAGS='-C link-arg=-L/usr/local/opt/xz/lib' CARGO_BUILD_JOBS=1 cargo clippy -p codegg --test search_backend_arg_mapping --test fake_eggsearch_mcp --test search_backend_eggsearch -- -D warnings
rtk proxy env LZMA_API_STATIC=1 RUSTFLAGS='-C link-arg=-L/usr/local/opt/xz/lib' CARGO_BUILD_JOBS=1 cargo clippy -p codegg --lib -- -D warnings
rtk cargo fmt --all -- --check
rtk git diff --check
rtk proxy env LZMA_API_STATIC=1 RUSTFLAGS='-C link-arg=-L/usr/local/opt/xz/lib' CARGO_BUILD_JOBS=1 scripts/verify.sh quick
```

### Results

- Test-target cargo check passed.
- Argument mapping: 15 passed.
- Fake eggsearch MCP compatibility: 28 passed.
- Wrapper/exposure compatibility: 9 passed.
- Research source structured-result compatibility: 5 passed.
- A strict targeted clippy pass and a strict `codegg` library clippy pass
  passed with no warnings.
- Formatting, diff hygiene, all quick-verification guards, and locked
  workspace cargo check passed.
- The unworkarounded normal test-link attempt was blocked by the host's
  pre-existing x86_64/arm64 `/opt/local` `liblzma` mismatch. The documented
  repository-built/static xz linker workaround made every focused test pass.
- The requested full workspace all-target clippy command was attempted; its
  post-fix run was terminated by the host with SIGTERM while compiling an
  unrelated `codegg-core` test target, without a diagnostic. The passing
  targeted strict clippy and library clippy runs plus quick gate provide the
  bounded substitute evidence; this is not an M007 code finding.
- No real eggsearch 0.3.9 process smoke was run because no configured
  eggsearch binary was available. The deterministic fixture covers the
  request contract and the opt-in smoke remains available outside CI.

## 5. Invariant review

- Full structured upstream data remains available through `value`; bounded
  text is a separate framed projection.
- Diagnostic/partial retrieval metadata cannot be mistaken for complete
  evidence: provider failures and retrieval state remain in the structured
  value, and MCP `isError` is retained as `success=false`.
- Stable IDs, warnings, trust markers, suggested fetches, next actions, and
  unknown additive fields remain data. Nothing executes next actions.
- Legacy aliases retain unambiguous translations; unsupported and conflicting
  input fails clearly before dispatch.
- Raw MCP tools remain hidden by default and no provider-specific HTTP path or
  CodeGG-owned persistent cache was added.

## 6. Failure and recovery review

Malformed types, invalid enum values, out-of-range controls, blank focus,
metadata-only focus, bypass/age conflicts, ambiguous repository locators, and
stale aliases are rejected before MCP dispatch. Existing timeout, URL
validation, MCP cancellation, output caps, and trust framing remain in the
shared path. Batch normalization does not drop per-item controls. No durable
state or lease is introduced, so restart, duplicate delivery, persistence
recovery, and rollback remain governed by the existing MCP/backend path.

## 7. Migration and compatibility review

No storage migration or configuration migration is required. Existing native
argument names remain stable; new names match the upstream provider-neutral
contract. The internal diagnostic request is explicit and not user-configured.
Older eggsearch servers that ignore additive fields remain covered by the
existing compatibility boundary, while the hardened fixture detects accidental
drift in current field names and ranges. Reverting `0cd35ab` restores the
pre-M007 adapter behavior without a data migration.

## 8. Security review

All new inputs are bounded and validated before external dispatch. Focus is
guidance for one fetch, not traversal. Cache controls do not create a second
cache or weaken the upstream tightening semantics. External content remains
trust-framed, structured metadata cannot grant execution authority, raw MCP
tools remain hidden, and secrets/auth/URL/process controls are unchanged.

## 9. Documentation and operations

- `architecture/mcp.md` documents the eggsearch 0.3.9 baseline, selected
  controls, internal diagnostic policy, structured-value/display split, and
  deferred provider-specific surface.
- The fake MCP fixture is the diagnostic contract for future adapter changes;
  focused tests show the exact command forms and expected counts above.
- No new network-dependent CI, scheduled smoke, compatibility matrix, or
  operational service was added.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Native host's default linker selects incompatible `/opt/local` arm64 xz libraries for x86_64 test binaries | Normal unworkarounded test links fail on this host | Use the repository-built/static xz workaround or a compatible host/toolchain; no M007 code change required |
| low | No configured eggsearch 0.3.9 binary for optional real-process smoke | Live binary evidence is unavailable in this environment | Run the opt-in smoke when a pinned binary is available; ordinary CI remains offline |

No critical, high, or medium M007 correctness, security, migration, or
resource finding remains.

## 11. Roadmap disposition

M007 is closed and M008 remains ready and independently executable. M007's
closure satisfies the hard M006 → M007 dependency; it does not alter the
independent M008 dependency graph or unblock the unrelated Architecture M009
and Runtime Safety C002 operational blockers.

## 12. Registry updates

The final registry update will:

- mark M007 closed in the active corrective roadmap and record this closure
  record plus implementation commit under recently closed work;
- restore the corrective roadmap status to active because M008 remains ready;
- keep M008 in the dependency-ready table with no new blocker;
- leave the two unrelated blocked plans unchanged after an explicit audit;
- update the roadmap milestone heading to show M007 closed.
