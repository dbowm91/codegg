# Eggsearch Evidence Workflows M001 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/eggsearch-evidence-workflows/001-evidence-fidelity-and-retrieval-state.md`

Source subsystem roadmap: `plans/subsystems/eggsearch-evidence-workflows-roadmap.md#5-milestones-and-handoffs`

Repository baseline reviewed: `b24a7be5`

Implementation commits:

- `6be801e3a02349630a153f4141e9a69dce789584` — preserve eggsearch retrieval evidence state

## 1. Executive finding

M001 is closed. Deep research now carries an explicit eggsearch retrieval
receipt alongside its existing source records. Complete empty results,
provider/tool failures, partial results, skipped network collection, truncated
text-only responses, and legacy responses with unknown completeness stay
distinct through coordinator results and generated research outputs. Existing
per-run `SourceRecord.id` values and persisted source-record shapes remain
unchanged.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Typed retrieval state reaches coordinator, run artifact and answer/report output | `ResearchSourceAdapter::collect_with_receipt`, `ResearchCoordinator::collect_sources`, `ResearchRunResult.retrieval_receipts`, `retrieval.json` / `ResearchBundle.retrieval_receipts`, and `render_retrieval_status`; synthesis regression test distinguishes complete-empty from failed-empty | pass | Status is structured and is not parsed from source notes. |
| Preserve upstream stable identity without replacing CodeGG IDs | Eggsearch receipt links existing source UUIDs to bounded upstream stable IDs; same-URL dedup test retains distinct stable IDs and collapses identical stable IDs | pass | Records without an upstream ID and non-eggsearch adapters retain URI deduplication. |
| Preserve nested publication dates | Eggsearch converter prefers `metadata.published_at`, parses RFC3339 and date-only UTC values, and falls back to legacy top-level fields; unit fixture asserts the date-only value | pass | No date is invented when fields are missing or invalid. |
| Distinguish complete no-match from failure and uncertainty | Unit status matrix covers complete-empty, failed-empty, all-provider-failed-empty, and unknown-empty; fake MCP confirms old response shape remains unknown | pass | `success=false` with valid structured sources is classified partial; no structured status is treated as complete. |
| Preserve warnings, trust markers, coverage, provider/group origin | Receipt unit fixture checks warning code, trust-marker names, coverage summary, failed provider, provider/group link, and source association | pass | Upstream markers remain descriptive; `SourceRecord` keeps CodeGG `external_untrusted`. |
| Keep network policy, structured-value precedence, and truncation behavior | Fake MCP network-budget test; structured-value conversion test with truncated display projection; text-only truncation test | pass | Full structured data takes precedence over display truncation; absent structured data with truncation remains explicitly truncated. |
| Compatibility and migration | `SourceRecord` serialization and storage shape unchanged; result and bundle receipt fields are additive and serde-defaulted; storage test covers old runs without `retrieval.json` | pass | No SQLite schema or migration change. |

## 3. Production implementation evidence

- `ResearchSourceAdapter` gained a default receipt-aware collection method, so
  existing local adapters retain their collection semantics.
- Eggsearch normalizes nested publication dates, retains bounded stable IDs,
  group/provider provenance and upstream trust-marker names, and captures
  retrieval state in a separate typed receipt.
- The coordinator gathers receipts, keeps distinct eggsearch stable IDs even
  when two records share a URL, and associates retained records with their
  receipt links after source-budget truncation.
- Research outputs render incomplete or unconfirmed retrieval as a coverage
  gap. A confirmed complete zero-result search is represented as complete with
  zero sources.
- The coordinator writes structured receipts to `retrieval.json`; older runs
  load into `ResearchBundle` with an empty receipt list.
- No external provider, tool execution path, permission, or SQLite schema was
  added or changed.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all
cargo check -p codegg --locked -q
cargo test -p codegg --lib research::sources::eggsearch -- --test-threads=1
cargo test -p codegg --lib research:: -- --test-threads=1
cargo test -p codegg --test fake_eggsearch_mcp -- --test-threads=1
cargo test -p codegg --test search_backend_eggsearch -- --test-threads=1
CARGO_BUILD_JOBS=2 cargo clippy --workspace --lib --locked --features server,plugins,lsp-test-support -- -D warnings -A deprecated
cargo clippy -p codegg --all-targets --locked -- -D warnings
scripts/verify.sh quick
git diff --check
```

### Results

- Formatting and `git diff --check` passed.
- Root crate check passed; final `verify.sh quick` also passed its locked
  workspace all-target check and all configured guards.
- Eggsearch research unit tests: 7 passed.
- Research library tests: 128 passed.
- Fake eggsearch MCP tests: 28 passed.
- Search backend eggsearch tests: 9 passed.
- Root crate Clippy across all targets: passed with `-D warnings`.
- Workspace library Clippy with the repository-supported server/plugins/LSP
  features passed. `-A deprecated` was needed for the unrelated existing
  `AtomicUsize::fetch_update` deprecation in `src/server/ws.rs` on this local
  toolchain.
- One intermediate test run found that trust-marker object iteration order was
  not the asserted order. Marker names are now sorted for deterministic
  receipts, and the final focused test run passed.

An attempted workspace all-target Clippy run with those features stops while
compiling the unrelated `tests/team_collaboration_m006_trajectory.rs` target:
its `CoreRequest::TurnSubmit` fixture is missing the `reasoning` field. The
repository explicitly disallows `--all-features` workspace sweeps because they
enable real-server tests; workspace library Clippy and the canonical quick
workspace check provide the bounded substitutes used here.

All evidence is local Linux evidence. No hosted CI or live eggsearch process
was used or claimed.

## 5. Invariant review

- Existing source UUID, run ID, URI and serialized `SourceRecord` contract are
  unchanged.
- Unknown and future response fields remain tolerated. Unknown/missing
  retrieval states remain indeterminate.
- A complete zero-result response is absence; failed, partial, skipped,
  truncated, and unknown collection is surfaced as a coverage gap.
- Valid source cards from unsuccessful or partially failed responses are
  retained as partial evidence.
- Local adapters continue to use the original `collect` contract through the
  trait's compatibility default.
- No persistent cache, background work, provider client, or permission change
  was introduced.

## 6. Failure and recovery review

MCP/tool errors become a bounded `tool_error` receipt instead of disappearing
as an empty result. Network-policy denial remains enforced before dispatch and
is represented as skipped in coordinator receipts. Display truncation only
marks retrieval truncated when no structured value is available. Source-budget
truncation updates the receipt and removes links for discarded records. No new
retry, cancellation, restart, or asynchronous task behavior was introduced.

## 7. Migration and compatibility review

No storage migration is needed. The added `ResearchRunResult` and
`ResearchBundle` receipt fields are serde-defaulted; prior persisted source
records remain readable and old runs load without `retrieval.json`.
Legacy eggsearch responses that lack a completion signal deliberately remain
unknown rather than being promoted to successful complete retrieval.

## 8. Security review

All source text and upstream metadata remain untrusted data. Upstream trust
markers and provenance are descriptive only; all network `SourceRecord`s keep
CodeGG's `external_untrusted` classification. Stable IDs, provider labels,
group labels, warning codes, and snippets are bounded and control characters
are removed from note labels. No `next_actions` are executed or surfaced as
authority.

## 9. Documentation and operations

- Updated `architecture/research.md` with the typed receipt and no-match
  semantics.
- `ResearchRunResult.retrieval_receipts` exposes structured diagnostics to
  callers; generated research outputs show retrieval state and bounded
  warnings.
- The final `scripts/verify.sh quick` passed, including repository boundary,
  authority, provider and workspace-check guards.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| Low | Feature-enabled workspace all-target Clippy is blocked by the unrelated missing `reasoning` fixture field in `tests/team_collaboration_m006_trajectory.rs` | The full feature-enabled integration-test lint target cannot compile on this baseline | Repair the fixture in its owning workstream; M001's root all-target Clippy, workspace library Clippy, focused tests, and canonical quick verification pass. |

## 11. Roadmap disposition

M001 is closed. M002 and M003 remain blocked on the separate search/eggsearch
legacy-retirement M001 release adoption. M004's evidence-fidelity dependency
is now closed, but it remains blocked on that same legacy-retirement M001.
M005 remains blocked on M002 and M004. No scope from the separate retirement
workstream was taken into this milestone.

## 12. Registry updates

- M001 moves from implementation/closure review to `closed` and is removed
  from the dependency-ready section.
- The eggsearch evidence workflows roadmap returns to `active` with M001
  closed and M002-M005 still blocked or sequenced.
- Blocker audit: M002 and M003 retain their legacy-retirement M001 blocker;
  M004's evidence M001 dependency is satisfied, but its remaining
  legacy-retirement M001 hard dependency keeps it blocked; M005 retains its
  M002+M004 dependencies. No plan becomes ready in this closure.
- The separate legacy-retirement M001 remains ready and is the next external
  dependency gate for M002-M004.
