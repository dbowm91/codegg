# TUI Editor Post-Closure Corrective C006 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-editor-correctness-usability/008-daemon-listing-admission-wire-budget-corrective.md`

Source subsystem roadmap:

- `plans/subsystems/tui-editor-correctness-usability-post-closure-corrective-addendum.md#C006`

Repository baseline reviewed: `301a5162d8ab5c8b9de2c3d19184466f60814d9a`
(branch `plans/tui-editor-correctness-usability-20261013`; preserves
`f5cc17f24` C003 close plus the C006 registration; no unrelated changes
disturbed).

Implementation commits:

- `7995f8bf0118bf18f785119c1777350a9c4eb6b2` — `tui-editor(c006):
  worker-owned listing admission and full-response wire cap`
  (worker-owned `OwnedSemaphorePermit`, per-daemon `DirectoryListingProbe`
  with test-only gate, full-response 512 KiB budget enforcer with
  binary-search trim to zero, 13-test `tests/directory_listing_c006.rs`,
  TUI empty-truncated test, ADR-0022 provenance note, `architecture/tui.md`
  + `architecture/protocol.md` + `docs/tui.md` updates)

## 1. Executive finding

C006 is implemented and closed. The two concrete C005 gaps are repaired
within the unchanged daemon-owned, read-only architecture:

1. Daemon single-flight admission survives handler cancellation: the
   permit is owned by the actual `spawn_blocking` worker and releases
   only when the blocking closure exits (error and panic unwind
   included). Aborting the async waiter leaves the live worker holding
   admission; a second request answers `Busy` with no second scanner
   (measured peak live workers stays 1 across aborts, panics, failures,
   and 100 contenders).
2. Every successful `CoreResponse::WorkspaceDirectoryList` fits
   `MAX_DIRECTORY_LISTING_RESPONSE_BYTES = 524288` bytes as a complete
   serialized response (discriminator + ids + status + examined +
   combined truncation reason + nodes), measured with the same
   `serde_json` serializer the socket/stdio transports use. Trimming
   removes trailing whole top-level nodes in prefix order down to zero:
   a sole 592,412-byte nested root is dropped to an explicitly truncated
   218-byte empty `Ready`, never emitted oversize. The 4096-node worst
   case trims to 524,286 bytes in 14 serializations (`O(log n)`, bound
   32).

ADR-0022 Option B is unchanged; its `2026-10-14` acceptance-date claim is
reconciled as chronologically unverified (see §5/§9) with no fabricated
approval and no rewrite of C005's historical closure.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| AC1: cancelled mid-scan cannot free admission; peak blocking scans ≤1 across cancels, errors, panics, 100 contenders; permit returns only after actual worker exit, no leak | `tests/directory_listing_c006.rs::aborted_handler_does_not_release_permit_before_blocking_worker_exit` (blocked worker + abort waiter + second Busy + release + third Ready; active/peak asserts 1), `::hundred_concurrent_clients_never_enter_second_blocking_scan` (100 contenders all Busy, peak 1), `::panic_and_failure_release_permit_when_worker_exits`, `::abort_before_worker_starts_does_not_leak_permit` | pass | Deterministic barriers/notifiers, no sleeps; `DirectoryListingProbe` counts actual worker entry/exit, not waiter counts |
| AC2: every actual serialized `CoreResponse::WorkspaceDirectoryList` ≤524288 bytes incl. metadata/discriminator; sole oversized subtree never emitted; all-roots-dropped is explicitly truncated | `::single_nested_root_over_512k_is_dropped_with_truncation` (592,412-byte root → 218-byte truncated empty), `::full_core_response_serialized_under_512k_with_metadata`, `::near_limit_unicode_and_escape_payload`, `::combined_truncation_reasons_are_counted`, `::all_roots_removed_is_visible_truncated_not_empty_success`, `::response_budget_enforcer_does_not_reserialize_quadratically` (4096 nodes → 524,286 bytes in 14 serializations), `::busy_and_unavailable_responses_respect_the_budget`, `::small_listing_keeps_original_order_content` + lib `core::daemon_documents` budget tests (3 pass) | pass | Full-response `serde_json` measurement; negative proof (pre-fix keep-one-root would emit 592,412 bytes) included |
| AC3: deterministic bounded trim; 4096/100000/depth/symlink limits intact; no covert schema or ADR contradiction | Prefix-order retention asserted in trim tests; `::authorization_and_containment_unchanged` + existing `directory_listing_c005` 12 pass (deep/node budgets, decoy, traversal, symlinks); `check_daemon_listing_authority.py` green; no DTO/auth-matrix change | pass | Recursive child pruning refused (would need ADR amendment); top-level-only policy preserved to zero |
| AC4: Busy/Unavailable/Ready-truncated/denied/unsupported remain distinct; file.read gate and no-local-fallback unaffected | `::authorization_and_containment_unchanged` (cross-binding `document_not_found`, outsider `project_not_found`, Viewer Ready, traversal `document_invalid_path`, symlink `Unavailable`, `DocumentOpen` reauthorization) + C005 suite 12 pass + TUI `daemon_errors_fail_closed_with_no_local_fallback` (existing, still green) | pass | Denials masked via `observe_private`, exactly like `DocumentOpen` |
| AC5: C005 closure preserved; ADR-0022 chronology resolved with verifiable provenance or visibly annotated, not backdated | ADR-0022 `## C006 provenance note` (cites `e37774a9` 2026-10-13 12:12:58 UTC vs `193a4441` 2026-10-13 12:16:08 UTC vs `2026-10-14` claim; no independent approval located; contract frozen, order unverified; escalated to maintainers); C005 closure file untouched | pass | Accepted ADR annotated additively, not rewritten; frozen Option B intact |
| AC6: failing-before/passing-after regressions, guards, commands, hosted runs, residuals, docs in closure | This record §4 (exact commands + counts), §9 docs, §10 lows; `architecture/tui.md` C006 subsection, `architecture/protocol.md` variant note, `docs/tui.md` cap/admission contract | pass | Hosted CI/E2E not run before closure — low findings per C001–C005 standard, not hidden |

## 3. Production implementation evidence

- Admission (`src/core/daemon_documents.rs::directory_listing`,
  `src/core/daemon.rs`, `src/core/daemon_construct.rs`): `try_acquire_owned`
  permit moved into the `spawn_blocking` closure (`let _permit = permit`
  inside the worker); no await between acquire and spawn. `Drop`/abort of
  the outer `JoinHandle` cannot release it; error return and panic unwind
  release at actual exit. `Semaphore(1)` per daemon retained; contention
  answers `Busy`; worker panic/failure answers sanitized `Unavailable`
  with no daemon paths.
- Instrumentation (`DirectoryListingProbe`, `DirectoryListingGate`,
  `WorkerActiveGuard`, `DirectoryListingGateBehavior` in
  `daemon_documents.rs`; `directory_listing_probe: Arc<…>` on
  `CoreDaemon`, constructed in `daemon_construct.rs`): `active`/`peak`
  updated on the worker thread under RAII (decrement on drop incl.
  unwind); `changed: Notify` + `wait_for_active` for deterministic tests;
  gate offers `BlockUntilReleased` (condvar, works on blocking threads),
  `PanicAfterEnter`, `FailAfterEnter`. Production never installs a gate.
  `take_gate` clones under lock; blocked workers count as active.
- Wire cap (`build_bounded_listing_response`,
  `build_bounded_listing_response_counted`, `serialized_listing_response_len`,
  `listing_response_fits_budget`, `combined_wire_reason` in
  `daemon_documents.rs`): measures `serde_json::to_string(&CoreResponse)`
  (same serializer as `CoreFrame::Response` JSONL transports). Fast path
  returns the untrimmed response when it fits. Otherwise binary-searches
  the longest fitting top-level prefix in `[0, n)` with one shared
  combined reason (walker notice + `listing trimmed to fit the
  524288-byte response budget`), hence monotone size in the retained
  count; `O(log n)` full serializations (measured 14 for 4096 nodes,
  bound 32 in tests). Trims to zero with `truncated=true`; empty-Ready
  overflow fails closed to `Unavailable`, then bounded `Error`. `Busy`
  shapes untouched and tiny.
- TUI (`src/tui/commands/file_tree.rs` tests only, no production change):
  new `c006_wire_cap_empty_truncated_stays_visible_not_empty_success`
  proves `listing_from_daemon` preserves wire-cap empty (`Ready` + empty
  + truncated + budget reason) distinctly from genuine empty success.
  `/files` and Ctrl+T share the same `FileTreeState`, so the distinction
  renders in both.
- Docs/guards: `architecture/tui.md` C006 subsection (worker-owned
  permit, full-response cap, trim-to-zero, `O(log n)`, provenance
  unverified); `architecture/protocol.md` variant note;
  `docs/tui.md` user contract (512 KiB complete-response cap, truncated
  empty vs empty project, cancel-safe admission, `r` to retry busy);
  ADR-0022 additive provenance note; `check_daemon_listing_authority.py`
  unchanged and green (single shared walker preserved).

## 4. Verification executed

Local only; hosted CI / Desktop E2E were not run before closure (low
findings §10, same standard as C001/C002/C004/C005).

### Commands run

```bash
cargo test -p codegg --lib core::daemon_documents --locked
cargo test --test directory_listing_c006 --locked
cargo test --test directory_listing_c005 --locked
cargo test -p codegg-protocol --locked
cargo test -p codegg --lib tui::file_tree --locked
cargo test -p codegg --lib tui::commands::file_tree --locked
cargo test --test file_tree_trajectory --locked
cargo test --test tui_panel_docking --locked
python3 scripts/check_daemon_listing_authority.py
python3 scripts/check_authorization_matrix.py
python3 scripts/check_tui_project_authority.py
python3 scripts/check_tui_editor_text_authority.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
```

### Results

- `core::daemon_documents` lib: 3 pass (2 budget + symlink gate).
- `directory_listing_c006`: 13 pass (4 admission: abort-holds-Busy,
  panic/fail release, abort-before-start no-leak, 100-contender peak-1;
  7 wire: sole-root drop, metadata, unicode/escape, combined reasons,
  empty-truncated visibility, order preservation, quadratic guard +
  Busy/Unavailable budget; 1 auth/containment; 1 top-level count
  includes all).
- `directory_listing_c005`: 12 pass (no regression: bounds, decoy,
  traversal, symlinks, auth, admission burst, reopen).
- `codegg-protocol`: 226 pass (prefix validation + DTO roundtrip intact).
- `tui::file_tree` lib: 31 pass (shared walker unchanged).
- `tui::commands::file_tree` lib: 14 pass (incl. new C006
  empty-truncated visibility + all C004 admission/parity/fail-closed).
- `file_tree_trajectory`: 4 pass. `tui_panel_docking`: 33 pass.
- Guards: daemon-listing authority OK; authorization matrix verified;
  TUI project authority passed; TUI editor text authority passed.
- `cargo fmt --check`: pass. Workspace Clippy `-D warnings`: pass.
- `scripts/verify.sh quick`: pass (incl. agent schema, core/client/
  desktop boundaries, sandbox, execution-ownership, TUI authority,
  route disposition, audit, scheduler bypass, config merge, models,
  provider wire/catalog/resilience, OpenAI endpoint, Eggwork routing,
  workspace check).

Measured wire bytes (same `serde_json` as transport, `CoreResponse`):

- Untrimmed sole nested root (100 subdirs × 20 files, 100-char padding):
  **592,412 bytes** — exceeds 524,288 by 68,124 (negative proof: pre-fix
  keep-one-root policy would emit it).
- Post-fix trimmed response for that tree: **218 bytes** (`Ready` +
  empty + `truncated=true` + wire reason) — fits with margin.
- 4096-node worst case: trims to **524,286 bytes** (2 under cap) in
  **14 serializations** (bound 32; naive pop-one-by-one would take
  ~4096).
- Actual worker peak: **1** across abort-while-blocked, panic, failure,
  and 100 concurrent contenders (`active` returns to 0; permit to 1).

## 5. Invariant review

- One text authority: listing carries names/paths/depth/kind only;
  `check_tui_editor_text_authority.py` green; walker reads metadata,
  never file bytes.
- One walk authority: `src/directory_listing.rs` remains the sole
  enumerator; TUI presentation delegates; guard pins it.
- Daemon-owned root + `DirectProject + FileRead`: re-verified in
  `::authorization_and_containment_unchanged`; TUI never walks locally;
  `DocumentOpen` still reauthorizes selected files.
- Single-flight until actual worker exit: proven by barrier abort/Busy
  tests with `active`/`peak` instrumentation, not waiter counts.
- Finite bounds preserved: depth 12, nodes 4096, examined 100000, names
  255B, symlinks never followed, full-response ≤524288B; truncated
  empty stays `truncated=true` with a bounded reason.
- No new protocol/authorization: additive behaviour only inside the
  frozen ADR-0022 contract; DTO shapes, matrix rows, family dispatch
  unchanged; recursive pruning refused without an ADR amendment.
- C005/C003/C004 closures immutable: none edited; this record is additive.

## 6. Failure and recovery review

- Normal: acquire → worker scans → worker exits freeing permit → bounded
  `Ready`.
- Contended: second request while worker alive answers `Busy` with
  `examined == 0` and no scan; proven with 100 contenders all `Busy`.
- Client abort/timeout: outer response abandoned, worker continues
  holding admission until exit; cancellation remains cooperative at
  filesystem checkpoints (no claim of interrupting a kernel syscall).
- Worker panic/I-O error/injected failure: worker-owned guards release at
  unwind/exit; caller sees sanitized `Unavailable`, never fabricated
  `Ready`; daemon serves the next request (`Ready` after both abnormal
  exits proven).
- Abort before spawn: handler-owned permit drops with no leak; `Busy`
  path spawns no worker (`peak` stays 0); next request `Ready`.
- Shutdown: no forced thread kill claimed; permit follows actual
  completion; TUI generation/request/epoch fences still discard late
  completions (existing C004/C005 handoff tests green).
- Too-large listing: deterministic longest fitting top-level prefix,
  explicit combined reason even at zero nodes; normal small listings
  byte-identical (order/content preserved).
- Cross-client: one daemon semaphore shared; project/workspace binding
  re-verified per request; no cancelled/stale data adopted by another
  tab (echoed `workspace_id` match + generation fences unchanged).

## 7. Migration and compatibility review

Additive only: no DB migration, no durable state, no DTO field change,
no new request/response variant. Old daemons fail the unknown variant at
deserialization → transport error → explicit unavailable message with no
local fallback (existing TUI test still green; C006 re-asserts
`Busy`/`Unavailable`/`Error` shapes stay tiny and within budget).
`Busy`/`Unavailable`/`Ready`-truncated/denied stay distinct wire shapes.
Local TUI uses the identical daemon path (in-process client); no
privileged local exception.

## 8. Security review

- Authorization before I/O on every path: central dispatch gates
  `DirectProject + FileRead`; handler re-verifies project/workspace
  binding and prefix containment daemon-side; team denials masked via
  `observe_private` (outsider `project_not_found` proven).
- Non-enumerability: cross-wired/unknown bindings answer
  `document_not_found`; `Unavailable` carries no daemon paths; TUI
  diagnostics expose counts only.
- Traversal/symlink/TOCTOU: prefix grammar rejects `..`/absolute/empty
  components pre-I/O; symlink components rejected on raw metadata at
  prefix and every walk level; walk never follows symlinks; special
  files skipped; long names skipped, not truncated.
- No `ProjectObserve`-implies-listing: matrix row requires `file.read`;
  observer-shaped principal denied in tests.
- No secrets in logs: worker/gate/probe expose counts only, never paths
  or entry names; truncation reasons are bounded static text plus the
  pre-existing walker notice.

## 9. Documentation and operations

- `architecture/tui.md`: C006 subsection (worker-owned permit,
  probe/gate instrumentation, full-response cap, trim-to-zero,
  `O(log n)`, provenance unverified).
- `architecture/protocol.md`: `WorkspaceDirectoryList` variant note
  updated to worker-owned permit + full-response cap.
- `docs/tui.md`: user contract updated (complete-response 512 KiB cap,
  truncated-empty vs empty project, cancel-safe admission, `r` retry).
- `plans/adrs/ADR-0022-daemon-authorized-workspace-directory-listing.md`:
  additive `## C006 provenance note` citing `e37774a9` (2026-10-13
  12:12:58 UTC), `193a4441` (2026-10-13 12:16:08 UTC), and the
  `2026-10-14` claim; order marked unverified, escalated to maintainers;
  frozen Option B untouched; C005 closure untouched.
- Telemetry remains counts/caps/cancellations/errors/durations only.
- Operator note: `Busy` → explicit `r` retry; wire-cap empty shows its
  truncation reason in `/files` and Ctrl+T (not an empty project);
  `Unavailable` → explicit error, no local fallback.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Hosted exact-SHA `CI / verify` not run on `7995f8bf` | Same residual as C001/C002/C004/C005; local `verify.sh quick` + clippy + focused suites green | Run hosted CI before release; no code impact expected |
| low | Hosted `Desktop E2E` not run on `7995f8bf` | No desktop-specific delta (daemon + TUI-pane mapping + docs); same residual as C001/C002/C004/C005 | Run built-app E2E on release qualification |
| low | Real two-physical-hosts smoke not run | Fake-transport decoy + same-path/different-root daemon tests remain the reproducible proof (per C005 §5E non-goal deferral) | Optional physical-host smoke on release |
| low | Background `@`-mention indexer still walks launch dir client-side | Separate index, explicitly deferred by C005 non-goals; untouched by C006 | Future corrective only if mention completion must be daemon-scoped |

No critical, high, or medium findings. No defect remains in landed code.

## 11. Roadmap disposition

Milestone C006 closed at `7995f8bf`. The corrective addendum's C006 row
moves to `closed`; historical C003 (closed `1e74aa19`), C004 (closed
`b9400c98`), C005 (closed `e37774a9`) are preserved unchanged. The
TUI-editor correctness/usability post-closure corrective workstream is now
C003–C006 all closed with no further corrective registered. The
`@`-mention indexer note above is deferred product work, not a corrective
pass on this milestone.

## 12. Registry updates

- `plans/implementation/tui-editor-correctness-usability/008-daemon-listing-admission-wire-budget-corrective.md`:
  `implemented` at `7995f8bf`.
- `plans/subsystems/tui-editor-correctness-usability-post-closure-corrective-addendum.md`:
  C006 row `closed` at `7995f8bf`; workstream C003 closed / C004 closed /
  C005 closed / C006 closed.
- `plans/registry.md`: active roadmap row updated to `active (C003–C006
  closed)` with C006 implementation + closure links; dependency-ready
  C006 row moved to `closed`.
- Unblock audit (per planning skill §Registry/Unblocking): swept
  `plans/registry.md` Blocked-work table and the corrective addendum's
  dependency graph. No registered plan lists C006, this roadmap's
  listing milestone, or ADR-0022 as a hard/interface dependency. Blocked
  rows name only Windows-sandbox C011/M004–M006, eggsearch M001/M002,
  architecture-convergence M009, runtime-safety C002, and
  tool-selection-advisor live/model lines — all unrelated to daemon
  directory listing. **Nothing is unblocked by this closure**; recorded
  here explicitly rather than left implicit.
