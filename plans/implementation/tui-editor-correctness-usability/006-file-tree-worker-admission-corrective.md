# TUI Editor Post-Closure Corrective C004 — File-Tree Worker Lifetime and Admission

Status: ready for handoff

Baseline: `plans/tui-editor-correctness-usability-20261013` at `0266de94780e6f69c8f0fe6f1cfb44ea0aeb4b56`; recheck HEAD before changes.

Source: `plans/subsystems/tui-editor-correctness-usability-post-closure-corrective-addendum.md` C004.
Predecessor: C002 `plans/implementation/tui-editor-correctness-usability/002-bounded-editor-render-and-tree-walk.md` and `plans/closure/tui-editor-correctness-usability/002-status.md`, implemented `bd891348f`. Preserve this historical closure.
Accepted ADRs: 0010, 0011, 0012. This corrective changes no protocol or authority.
Class: concurrency / resource-bounds invariant.

## 1. Objective

Make the claim “at most one file-tree scan executes” true for the **underlying blocking work**, not merely the registered asynchronous waiter. Admission, supersession, cancellation, draining and shutdown must account for `spawn_blocking` work that cannot be stopped by aborting its Tokio waiter. Preserve the existing 4096-node, 100000-examined-entry, depth, symlink and local-only guards until a daemon-authorized listing succeeds it.

## 2. Source evidence, failure scenario, and old coverage gap

`src/tui/commands/file_tree.rs::rebuild` invokes `app.task_registry.cancel_kind(TuiTaskKind::FileTree)` and immediately spawns a new registered async waiter which internally starts `tokio::task::spawn_blocking`. `TuiTaskRegistry::cancel_kind` aborts and removes the outer Tokio task; it does not join or abort the spawned blocking closure. `src/tui/app/state/file_tree.rs::begin_rebuild` sets the prior cooperative flag, and `build_tree_with_cancel` polls this flag between entries. That is useful cancellation, but not mutual exclusion while an entry syscall or directory enumeration remains blocked.

Consequently a second `spawn_blocking` can begin before the first actually returns. Repeated rebuilds can create multiple concurrent blocking scans despite `a_hundred_rapid_rebuilds_keep_one_live_task` reporting one *registry* task. The original test asserts registry counts and flags, not actual blocking worker entry/exit or maximum observed concurrent scans. The closure's “one live task” is defensible for registered waiters; the stronger “hard limit one blocking worker” is not established.

There is no unconditional wall-clock cancellation bound for `read_dir`/`DirEntry::file_type` on slow, hung, remote-mounted or adversarial filesystems. “Exits within one entry” means within one **completed** entry and is not a time guarantee. No new untrusted directory listing protocol is needed to fix admission.

## 3. Invariants and scope

- The UI event loop never directly walks the filesystem. Blocking work executes on a bounded blocking pool; never `std::fs::read_dir` on the async worker or renderer.
- A superseded scan can continue running only as an accounted, non-authoritative, finite-budget prior operation; there can be **no unbounded pileup** of simultaneously running blocking scans.
- Define explicitly either a global single active blocking scan (preferred) or a small fixed semaphore concurrency cap; document which and why. Cancelled tasks do not release scan capacity until the blocking work exits.
- Every scan is fenced to its request/generation/project/tab/root; results from superseded and wrong-project scans never display in the tree or `/files` dialog.
- Scan bounds remain 12-deep, 4096 retained nodes, 100000 examined entries (including ignored or symlink entries), 255-byte names, symlink never followed. Partial/truncated/cancelled states are truthful.
- No change to document text ownership, workspace root authorization, open-editor seam, or remote locality. C005 separately resolves trusted daemon-vs-local directory authority.
- No promises of hard termination within milliseconds of a hung kernel filesystem operation; busy must be explicit and bounded.

## 4. Required changes

**Concurrency owner:** Introduce a small explicit `FileTreeScanCoordinator` / per-TUI scan admission record rather than relying on `TuiTaskRegistry` count as a proxy for `spawn_blocking` completion. The admission permit is owned by the blocking work itself (RAII until completion), not the abortable waiter. For strict single-flight, each newer request supersedes old intent immediately but delays new blocking enumeration until the old worker's permit is released. Alternative implementation is queued coalesced latest-request execution under a single long-lived worker, avoiding creation of unbounded pending Tokio tasks. It MUST not queue every refresh.

**Cancellation:** A scan captures an immutable root, generation and cancellation token. Supersession, project switch, close and shutdown set old token before queuing successor. Check cancellation at each feasible filesystem boundary. Retire permit only on actual exit; ensure panics unwind via RAII; record errors without presenting an invalid listing. If shutdown cannot await a kernel-blocked worker, account for the limit and report a bounded truthful shutdown policy; do not detach unbounded workers or claim they were joined.

**UI:** The newest requested root/listing has one `Loading` or explicitly `Waiting for prior scan to stop` state; errors and timeout/resource-busy conditions are visible and do not become “empty workspace”. Do not admit work against wrong project after delayed permit acquisition: revalidate scope/epoch before scan and again before display.

**Tree traversal:** Preserve cancellation/examined-entry validation. Fix `entries.flatten()` cost accounting if discovered in adversarial tests: errors returned by iterator still consume iteration work, and cancellation/budget should not be bypassed by long streams of errors; test and bound them as needed. Document inability to preempt single blocking syscalls.

**Storage/protocol/security:** none. Never change file.read authorization or use process cwd as root; do not extend local listing to remote mode.

## 5. Ordered work packages

A. Reproduce false one-worker claim with an **instrumented injected blocking enumerator** that increments active worker count on entry and waits on a test barrier. Begin A; supersede it with B, C, and 100 rebuild calls while A remains blocked. Observe previous code's `peak_active_workers > 1` despite `task_registry` showing at most one, and record the before evidence. Avoid a test that depends on wall-clock sleeps.

B. Design tiny single-flight coordinator / latest-only pending request. A worker's permit must stay held from actual scan entry to exit; cancelled Tokio waiter does not release it. Do not spawn a new blocking operation until previous exits. Ensure at most one pending *intent*, not unbounded `JoinHandle` or queued futures. Integrate with `file_tree_state` generations and existing `TuiTaskKind::FileTree` lifecycle.

C. Test panic, immediate cancel-before-start, cancel-during-blocking, worker release, repeated rebuild, switch from workspace A to B, editor close, global shutdown, no command channel, filtered-only directory and unreadable entry storms. Prove late results fail the generation/root/request fence. Preserve `/files` and Ctrl+T consumers.

D. Add diagnostics (`running_scan_count`, `queued_latest`, `superseded_count`) with no private path or file contents logged; expose clear operator error if scan cannot start within the bounded admission policy rather than silently allocating more workers.

E. Run the original file-tree/editor regression suites, static guards, broad quick and hosted exact-head CI. Add a deterministic barrier stress test to CI and document any unsupported real-filesystem interruption case. If a daemon-owned listing path under C005 will replace the local worker, keep the admission component small/reusable or retire it cleanly during C005; no substantial temporary framework.

## 6. Failure/restart/contention semantics

Aborting outer waiter never implies inner worker stopped. A prior worker holding a permit prevents more worker creation until it exits; newer intent supersedes older requests and coalesces. On cancellation during a blocked directory syscall, UI remains responsive and reports waiting/disabled; don't busy spin or allocate additional scans. On project switch, the previously requested root cannot reappear; stale completed data never shown. TUI restart loses ephemeral tree state; no durable task or result retained. On shutdown, signal cancellation once and report if a worker cannot be joined; never claim a deadline not enforced by the OS.

## 7. Compatibility/migration

No public DTO, DB migration, authorization or external config. Local tree ordering, status text except accurate busy/truncation details, directory expansion and both UI entry points remain. No remote browsing is added in this plan. Preserve all existing resource caps.

## 8. Tests and verification

- Unit: barrier-controlled active/peak blocking count (must be ≤1 for selected policy), release only on true blocking completion, supersession queue length ≤1, cancellation flag immediately set, worker panic permit release, examined errors and node budget.
- Integration: ``/files`/Ctrl+T new root listing after held A cancellation; B wins, A discarded; 100 refreshes while worker blocks never spawn 100 tasks or second blocking worker.
- Shutdown/restart: rooted worker not reintroduced after shutdown; explicit busy behavior; event loop still processes input; a long syscall is not reported as forcibly cancelled.
- Security: arbitrary symlink, ignored-entry flood and root-scope checks unchanged; no direct file-content reads.

~~~bash
cargo test --lib tui::file_tree --locked
cargo test --lib tui::app::state::file_tree --locked
cargo test --lib tui::commands::file_tree --locked
cargo test --test file_tree_trajectory --locked
cargo test --test tui_panel_docking --locked
python3 scripts/check_tui_editor_text_authority.py
python3 scripts/check_tui_project_authority.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
scripts/verify.sh quick
~~~

Add a new exact blocking-worker-count test and record the observed peak. Verify test names against HEAD before running. Hosted CI exact SHA is required for final strict closure; Desktop E2E remains relevant to shared UI boundaries but does not prove a TUI blocking scan.

## 9. Acceptance criteria

1. A held first blocking worker plus 100 rebuild requests yields at most one active worker (or the explicitly specified fixed cap) and at most one queued latest intent, proven by instrumentation rather than registry count.
2. Cancelling/aborting waiter does not free admission until blocking work actually returns; no unbounded Tokio blocking work accumulates.
3. The event loop remains responsive and gives explicit busy/error/truncated states while work is held.
4. Late A result after switching to B is not shown; ``/files` and Ctrl+T see only the latest valid tree.
5. Original depth/node/examined/symlink/security caps pass; cancellation and shutdown claims match measured semantics.
6. Scope, static guards, focused/broad verification, exact implementation SHA, hosted CI and operational caveats are recorded.

## 10. Stop conditions

Stop if a proposed fix requires thread killing, trusting `AbortHandle` as blocking-worker termination, unbounded spawns, directory reading from render/event loop, or an unreviewed new remote filesystem protocol. Do not conflate this local worker lifetime with C005's separate cross-host authority problem.

## 11. Documentation / closure

Update `architecture/tui.md`, `docs/tui.md` and the active corrective registry. Create `plans/closure/tui-editor-correctness-usability-post-closure-corrective/004-status.md` referencing original C002 closure, exact before/after worker-peak metrics, mixed cancel/reader errors, project switch, shutdown, CI, regressions and residual severity. Do not edit original closure file to conceal the false hard-limit inference.

## 12. Handoff

This is an independent, dependency-ready corrective. Prioritize deterministic concurrency instrumentation and latest-only scan admission, not a new general task scheduler. Follow-up C005 will relocate directory listing authority to the daemon if ADR-0022 is accepted.
