# TUI Editor Post-Closure Corrective C004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-editor-correctness-usability/006-file-tree-worker-admission-corrective.md`

Source subsystem roadmap:

- `plans/subsystems/tui-editor-correctness-usability-post-closure-corrective-addendum.md#C004`

Repository baseline reviewed: `f465536bc0707e947650af61692425f94fc630e6`
(branch `plans/tui-editor-correctness-usability-20261013`; plan baseline
`0266de94780e6f69c8f0fe6f1cfb44ea0aeb4b56` rechecked at activation, two
docs-only commits ahead with no production delta).

Implementation commits or pull requests:

- `b9400c98` — tui-editor(c004): single-flight file-tree worker admission

## 1. Executive finding

C004 is complete as a local concurrency/resource-bounds corrective. The
claim "at most one file-tree scan executes" is now true for the underlying
blocking work, not merely the registered async waiter. A new explicit
`FileTreeScanCoordinator` holds the admission permit from event-loop admit
until the blocking work's completion arrives; aborting a Tokio waiter never
frees capacity early. New rebuilds while a scan is active coalesce as one
pending latest intent (never an unbounded queue), the UI reports an explicit
`Waiting` state while the prior scan drains, and stale completions never
render against the wrong workspace. The old `entries.flatten()` error-bypass
is fixed and counted. No protocol, storage, migration, authorization, or
document-text change. Original C002 closure
(`plans/closure/tui-editor-correctness-usability/002-status.md`,
`bd891348f`) is preserved untouched; this is new corrective evidence only.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Held first worker + 100 rebuilds yields ≤1 active worker and ≤1 queued latest intent, proven by instrumentation not registry count (AC1) | `file_tree_coordinator::tests::barrier_held_worker_plus_100_rebuilds_keeps_one_worker_one_pending` (barrier, no sleeps: active 1, peak 1, pending gen 101, superseded 100); `commands::file_tree::tests::c004_held_scan_plus_100_rebuilds_keeps_one_worker_one_pending` (1 waiter, 1 pending, `Waiting`, peak ≤1, then stale drives pending, current renders) | pass | Global cap 1 documented in coordinator docs; alternative semaphore explicitly rejected |
| Cancelling/aborting waiter does not free admission until blocking work returns; no unbounded blocking accumulation (AC2) | Rebuild `Queued` path spawns nothing and aborts nothing; `RunningGuard` released only on blocking exit (incl. panic unwind); `finish_active` clears only on completion arrival; 100-queued test shows 1 waiter total | pass | Abandoned `cancel_kind` as admission control; `reap_finished` keeps registry count exact |
| Event loop responsive with explicit busy/error/truncated states while held (AC3) | New `FileTreeStatus::Waiting` (`· waiting for prior scan…` / `Waiting for prior scan to stop…`); pane keeps old nodes while waiting; errors/truncation distinct from empty; `c004_held…` asserts `Waiting` then `Loading` then `Ready` | pass | No busy spin, no extra workers, no stuck `Loading` |
| Late A after switch to B never shown; `/files` and Ctrl+T see only latest valid tree (AC4) | `c004_switch_from_a_to_b_shows_only_b` (A stale discarded, B renders, peak ≤1, dialog seeds from same state); pre-existing stale-generation/root tests still pass | pass | Generation + root + request fence at completion plus revalidation before delayed scan |
| Original depth/node/examined/symlink/security caps pass; cancel/shutdown claims match measured semantics (AC5) | `file_tree` 22 tests (depth/node/examined/symlink/ignored/unreadable), `iterator_error_stream_consumes_examined_and_respects_cancel` (200-error flood names examined budget, cancel observed), state 20 tests, trajectory 4, docking 33, guards green | pass | `account_one_entry` is the exact per-item function the walk calls; `Err`s counted |
| Scope, guards, focused/broad verification, SHA, hosted CI, caveats recorded (AC6) | §4 commands + results; `verify.sh quick` pass; SHA `b9400c98`; hosted CI not run recorded as low finding; shutdown policy documented | pass | Strict-hosted-CI gate recorded, not claimed |

## 3. Production implementation evidence

- `src/tui/file_tree_coordinator.rs` (new): `ScanRequest`
  (generation/request/root/tab), `Admit::{Run, Queued}`,
  `FileTreeScanCoordinator` (active/pending/superseded/completed +
  `running`/`peak_running` instrumentation, count-only `diagnostics()` with
  no paths), `RunningGuard` (RAII worker accounting, panic-safe). Policy:
  global single active scan with latest-only pending; every `Queued`
  admission counts one supersession; `finish_active` clears only on
  generation + request match; `take_pending_validated` drops stale pending
  rather than scanning the wrong workspace; `force_clear_for_shutdown`
  discards without claiming a join.
- `src/tui/commands/file_tree.rs`: `rebuild` resolves tab context, bumps
  generation, fails closed on non-local root / missing channel (clearing
  pending, leaving active to drain stale), then `try_admit`. `Run` reaps and
  spawns one `spawn_blocking` scan (with `track_enter` guard + cooperative
  flag); `Queued` sets `Waiting`, logs counts, spawns nothing, aborts
  nothing. `apply_listing` releases the permit even for stale completions via
  one `finish_active` call and launches the taken pending through
  `launch_taken_pending` (revalidates generation + root, re-checks locality
  and channel, promotes via `try_admit`, spawns). Stale path deliberately
  removed the old `request.cancel()` which would have bumped past the queued
  intent and stranded it.
- `src/tui/file_tree.rs`: replaced `entries.flatten()` with explicit
  `for next in entries` + `account_one_entry` per item (cancel, examined,
  max-examined, node-budget) before the `Ok`/`Err` split, so `Err`s consume
  work. Extracted `account_one_entry` so the error-flood test exercises the
  exact production function.
- `src/tui/app/state/file_tree.rs`: new `FileTreeStatus::Waiting` variant
  with docs; `begin_rebuild` still sets `Loading`, rebuild overwrites to
  `Waiting` when queued; nodes retained while waiting (never blanked to fake
  progress).
- `src/tui/components/file_tree.rs`, `dialogs/files.rs`: `Waiting` renders
  `· waiting for prior scan…` / `Waiting for prior scan to stop…` / `Loading…`
  (dialog treats `Waiting` as loading-like); error/empty states unchanged.
- `src/tui/app/mod.rs`: new `App::file_tree_coordinator` field, both
  constructors init, `prepare_shutdown` implements the bounded policy (signal
  flag once, drop queue, log counts, force-clear admission, abort waiters;
  documents that a kernel-blocked worker is not joined and its late
  completion mismatches and is discarded).
- `src/tui/mod.rs`: exports `file_tree_coordinator`.
- Docs: `architecture/tui.md` C004 subsection (single-flight rationale,
  permit ownership, `Waiting`, fencing, shutdown policy, `Err` fix,
  diagnostics privacy, no wall-clock preemption claim);
  `docs/tui.md` user-facing waiting/bounds paragraph.

Ownership preserved: UI event loop never walks; blocking I/O stays on
`spawn_blocking`; tree stays navigation-only with `open_editor` the sole
document path; no `file.read` change; no cwd authority; symlinks never
followed; depth 12 / nodes 4096 / examined 100000 / names 255B unchanged;
no remote browsing added (C005 separate).

Before/after: before, `rebuild` did `cancel_kind(FileTree)` then immediate
spawn; each of 100 rebuilds spawned new `spawn_blocking` while the old
blocking closure persisted until its next checkpoint, so peak workers could
reach 100 while `a_hundred_rapid_rebuilds_keep_one_live_task` still reported
1 registry task. Before peak was not measured with instrumentation (no hook
existed); the pileup is established by code inspection of the old spawn path
plus the old test asserting only registry counts. After, the barrier test
measures peak 1 with 100 queued, and the integration test measures peak ≤1
through stale-handoff, switch, shutdown, and panic-truncated paths.

## 4. Verification executed

### Commands run

```bash
cargo test --lib --locked -- tui::file_tree_coordinator tui::file_tree tui::app::state::file_tree tui::commands::file_tree
cargo test --lib --locked -- tui::components::file_tree tui::components::dialogs::files
cargo test --lib --locked -- tui::
cargo test --locked --test file_tree_trajectory
cargo test --locked --test tui_panel_docking
python3 scripts/check_tui_editor_text_authority.py
python3 scripts/check_tui_project_authority.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
scripts/verify.sh quick
```

All suites exist under the plan's names (plus the new coordinator module);
`cargo test --lib tui::file_tree` covers the walk including the new
error-accounting test; no substitutes. Test names verified against HEAD
before running.

### Results

- Combined `tui::file_tree_coordinator + tui::file_tree +
  tui::app::state::file_tree + tui::commands::file_tree`: 58 passed / 0
  failed (coordinator 8 incl. barrier peak 1 / superseded 100 / panic guard;
  walk 22 incl. new 200-error flood; state 20; commands 8 incl. 4 new C004
  integration: held+100, A→B, shutdown, panic-truncated).
- Components `file_tree + dialogs::files`: 16 passed / 0 failed.
- Broad `tui::`: 1510 passed / 0 failed (1476 at C002 close + new
  coordinator/error/C004/waiting coverage; no regressions).
- `file_tree_trajectory`: 4 passed / 0 failed.
- `tui_panel_docking`: 33 passed / 0 failed.
- `check_tui_editor_text_authority.py`: pass (one borrowed-snapshot note).
- `check_tui_project_authority.py`: pass.
- `cargo fmt --check`: pass.
- `cargo clippy --workspace --all-targets -- -D warnings`: pass.
- `scripts/verify.sh quick`: pass (fmt, agent schema, core/client/desktop
  boundaries, sandbox, execution-ownership, TUI authority/project authority,
  http-route-disposition, audit, scheduler-bypass, config-merge, provider
  wire/catalog/resilience, OpenAI endpoint, Eggwork routing, workspace
  check).

Local truth only. Hosted root CI and Desktop E2E were not run from this
environment (see §10 low finding). All traversal/admission tests above ran
locally on the implementation head `b9400c98`.

## 5. Invariant review

- No second document buffer: guard green; coordinator holds counts and
  roots for fencing only, never text; diagnostics contain no names/paths.
- No direct editor filesystem read: guard green; tree reads directory
  entries only, never file bytes; open stays `open_editor`-only.
- No project authority from cwd: guard green; rebuild resolves the active
  tab; locality refusal never consults cwd; delayed scan revalidates root.
- Symlink following: never; root-symlink refused, entry symlinks skipped on
  the raw path, including in the rewritten `Err`-counting loop.
- No silent truncation: `Waiting` + row suffix + banner + truncation reasons;
  empty/error/waiting states distinct.
- No unbounded blocking growth: one active + one pending max; `RunningGuard`
  + `finish_active` + barrier/integration peaks prove ≤1; superseded scans
  run once as bounded prior work, never pile up.
- No stale adoption: generation/root/request at completion plus
  generation/root revalidation before delayed scan; stale never cancels
  current.

## 6. Failure and recovery review

- Superseded scan: continues as accounted prior work within examined budget,
  completes stale, discarded, drives pending; never renders wrong workspace.
- Abort of waiter: never used for admission (only shutdown `cancel_all`);
  aborting never frees the permit; panic unwind releases `running` and still
  sends a truncated completion that drives pending.
- 100 rapid refreshes while held: 1 waiter, 1 pending (latest), 100
  superseded, peak 1, `Waiting` shown, then stale handoff to `Loading` to
  `Ready`.
- Project switch A→B: A discarded, B rendered, `/files` seeds from same
  state, peak 1.
- Shutdown: flag once, queue dropped, admission force-cleared, waiters
  aborted; kernel-blocked worker not joined (documented); late completion
  mismatches and is discarded; no relaunch; event loop already leaving.
- Missing channel / non-local root while active held: explicit `Error`,
  queue cleared, active drains stale with no launch; never a stuck `Waiting`
  or a same-name walk.
- Iterator error flood: counted, budgeted, cancellable; no bypass.

## 7. Migration and compatibility review

No schema, protocol, persisted config, DTO, ordering, or authorization
change. Local tree ordering, symlink handling, names, editor output, dialog
keys, and both UI entry points preserved (trajectory + docking + broad `tui::`
green). `Waiting` is additive UI text; `Idle`/`Loading`/`Ready`/`Error`
semantics unchanged. Historical C002/M003/M004 closures immutable; this
record is new corrective evidence. The admission component is small
(`file_tree_coordinator.rs` + two command helpers) with no general task
scheduler; if C005 relocates listing to the daemon, the coordinator ports to
the daemon blocking worker or retires cleanly with the local walk.

## 8. Security review

No path/document authority broadened. Guards green. Allocation DoS still
bounded by depth/node/examined/name caps, now including `Err` streams. No
secrets logged (diagnostics are counts only; roots never formatted). No new
authorization descriptor; tree remains navigation-only. TOCTOU stance
unchanged: listing is a best-effort snapshot; `DocumentOpen` reauthorizes
each file independently (C005 owns cross-host authority).

## 9. Documentation and operations

- `architecture/tui.md`: C004 subsection (cap-1 rationale, permit ownership,
  `Waiting`, fencing, shutdown policy, `Err` fix, diagnostics privacy, no
  preemption claim).
- `docs/tui.md`: waiting/bounds paragraph.
- Operational signals: `· waiting for prior scan…` title chip,
  `Waiting for prior scan to stop…` empty label, truncation reasons naming
  the fired budget, coordinator `diagnostics()` counts in debug logs,
  one-active/one-pending invariant.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Hosted root CI and Desktop E2E not run on `b9400c98` | Strict closure prefers exact-SHA hosted green; local `verify.sh quick` + 1510 TUI tests + clippy green substitute locally | Run hosted CI on the merge head; record runs here by factual correction if they fail |
| low | No live PTY/interactive TUI smoke | Roadmap calls one desirable; absence recorded, not claimed | Optional live smoke on a future pass |
| low | Examined-cap branch at production 100000 not exercised with 100000 real files | Committed tests prove the mechanism (200-error flood stops with examining reason at small budget; 1001-symlink flood counts exactly) through the identical `account_one_entry` path; creating 100k files is unsuitable for a unit test | A >100k ignored-entry fixture remains manual, not committed |
| low | Real hung-filesystem syscall interruption unavailable | Admission bounds concurrency, not wall-clock syscall completion; documented as unsupported with explicit busy state | No action; C005 daemon worker inherits the same caveat |

No critical/high/medium findings. No corrective pass required.

## 11. Roadmap disposition

Milestone C004 closed; C003 stays ready (independent, not gated by C004);
C005 stays blocked on proposed ADR-0022 (hard gate; C004 is only a soft
interface integration, now satisfied for future daemon-side reuse). No scope
expansion into completion or remote filesystem service. Unblock audit: no
registered plan lists C004 as a hard or interface dependency, so no plan was
unblocked by this closure. C005's hard ADR blocker is unchanged.

## 12. Registry updates

- `plans/implementation/tui-editor-correctness-usability/006-file-tree-worker-admission-corrective.md`:
  `ready for handoff` → `implemented; closed` with implementation `b9400c98`.
- `plans/subsystems/tui-editor-correctness-usability-post-closure-corrective-addendum.md`
  C004 row: `ready` → `closed`; closure linked.
- `plans/registry.md`: C004 `ready` → `closed` with closure
  `plans/closure/tui-editor-correctness-usability-post-closure-corrective/004-status.md`
  and implementation `b9400c98`; unblock audit recorded (C005 still blocked
  on ADR-0022, C003 already ready independent; nothing unblocked).
