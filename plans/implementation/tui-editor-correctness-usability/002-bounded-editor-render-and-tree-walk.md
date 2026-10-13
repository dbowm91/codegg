# TUI Editor Correctness/Usability C002 — Bounded Rendering and Tree Scheduling

Status: implemented; closed

Implementation: `bd891348f`
Closure: `plans/closure/tui-editor-correctness-usability/002-status.md`

Baseline: `main` `6b611b5ca7f019783a91f04530d5ae56124f7ee9` (2026-10-13); re-inspect before implementation.
Roadmap: `plans/subsystems/tui-editor-correctness-usability-roadmap.md` C002.
Closed predecessors: M005 document core, M006-A editor (`plans/closure/desktop-frontend-ide-foundation/005-status.md`), M006-D file tree (`006-status.md`).
ADRs: accepted 0010, 0011, 0012. No new ADR: locality-preserving performance correction.
Classification: correctness / infrastructure.

## 1. Objective

Bound actual editor frame work (bytes scanned, allocations and Unicode traversal), not merely output rows, for pathological multi-megabyte logical lines; isolate file-tree blocking I/O from Tokio workers and give traversal finite examined-entry and cancellation budgets.

## 2. Dependencies and current evidence

`src/tui/components/editor.rs::line_slice` reads an entire logical line before horizontal clipping. `line_spans` reads up to 16 KiB, while `src/tui/editor.rs::longest_visible_width` measures complete visible lines; a single enormous line can dominate the frame. Existing `per_frame_work_is_bounded_by_the_viewport_not_the_document` only tests the number of rendered rows.

`src/tui/commands/file_tree.rs::rebuild` calls synchronous `build_tree` from a `tokio::spawn` async task. Task abort cannot preempt synchronous filesystem work already executing in its poll. Its no-channel fallback walks synchronously on the caller. `src/tui/file_tree.rs::walk` has depth=12/node=4096 bounds but decrements the node budget **only for accepted entries**. Millions of ignored/symlink/special/failed entries may still be inspected unboundedly. Current tree tests do not establish an examined-entry budget.

## 3. Invariants

No second document buffer, direct editor filesystem read, project authority inferred from cwd, symlink following, silent truncation or fabricated cursor position. Preserve keyboard, path and document attachment behavior, including both the Ctrl+T pane and `/files` dialog. Tree is navigation-only; `open_editor` remains the authorized document-opening path. No renderer I/O. No unbounded blocking worker growth. No unsupported remote-root local walk or masquerading local root.

Out: soft wrap, arbitrary multi-buffer panes, remote directory protocol, filesystem watcher, full text-core redesign, syntax engine and LSP completion.

## 4. Required changes

**Core/DTO/storage:** unchanged. Snapshot remains the only text source. Presentation metadata caches are allowed only if bounded, keyed by attachment+revision and not copies of text.

**Renderer:** instrument per-frame maximum `read_bytes` request, total text bytes, column-seeking scans and transient allocation. Set explicit measurable budgets; qualification targets are ≤256 KiB source bytes examined per frame and ≤32 KiB per visible logical line, with a bounded fallback when a line or horizontal offset exceeds them. These targets may be tuned with evidence but must be explicit and verified against giant lines, not merely assert a wall-clock threshold. Support tabs, Unicode wide glyphs and combining characters; preserve cursor/selection integrity. Avoid allocating/slicing the entire logical line. If needed, amortize display-column lookup via bounded revision-keyed non-text checkpoints outside hot rendering. On exhausted budget visibly indicate partial line/too-long state rather than dropping text silently.

**Traversal:** add examined-entry budget separately from retained-node budget, retaining depth/name limits. Stop scanning even if all entries are ignored or symlinks. Keep a truthful truncation reason. Move blocking directory work to `spawn_blocking` or dedicated bounded blocking executor, and use cooperative cancel flags checked at enumeration checkpoints. The UI must never synchronously walk in the missing-command-channel fallback; return explicit unsupported/error instead. Generation/root/tab validation remains at completion.

**Remote locality:** where active workspace is not proven local to TUI, explicitly refuse file-tree listing instead of treating a same-name local path as authoritative. Do not introduce a new listing protocol inside C002.

**Authorization:** unchanged, symlinks never followed; update text-authority guard scope if helper code is moved.

## 5. Ordered work packages

A. Instrument and reproduce a 1–4 MiB single-line document with cursor at start and far right, 80x24 and 1x1 terminal, tabs and multi-byte characters. Record scanned bytes/calls. Create a directory with >100k ignored or symlink entries and <=4096 accepted nodes; demonstrate excessive scan before fix.

B. Refactor viewport and text-slice helpers to bounded chunk reads/column measurement, accounting for line width and viewport position; maintain old rendering for ordinary short lines and meaningful degradation for huge lines. Test actual input cost, not only visible rows.

C. Add independent examined-entry cap and result metadata. Isolate directory I/O on blocking executor, cooperative cancellation on rescan/tab change/shutdown, and hard limit concurrent blocking scans; do not let aborted async waiter strand indefinite scanning.

D. Integrate with `/files`, Ctrl+T tree, editor and pane focus; test delayed reads, switch/restart, narrow terminals and high-frequency input while scan is blocked. Add performance counters that never log file contents.

## 6. Failure, cancellation, restart and contention

A scan cancelled or superseded produces no complete authoritative listing; an older root never replaces a newer tab's view. Blocking executor tasks must finish within the cooperative checkpoint budget even after async abort. Repeated refreshes cannot accumulate unbounded workers. On line-budget exhaustion, editor state remains intact and displays a warning, never drops drafts or reads disk outside controller. Shutdown cancels active scans.

## 7. Compatibility/migration

No schema, protocol or persisted config change. Preserve local file-tree ordering, symlink handling, names and normal editor output. Document remote workspace listing as unsupported until a daemon-authorized transport exists. If huge-line motion is limited by budget, describe exact behavior in UI/docs and keep normal-file behavior unchanged.

## 8. Tests and verification

Tests: instrumented read-byte budget, extreme line at left/right/middle, column seeking with tabs/Unicode, tiny terminal, line-state warning, existing normal-file golden frame, large ignored/symlink directory, accepted-node cap distinct from examined-entry cap, unreadable subdir, traversal loop, async cancellation, 100 rapid refreshes without unbounded workers, project switch race, remote-root fail closed.

~~~bash
cargo test --lib tui::editor --locked
cargo test --lib tui::file_tree --locked
cargo test --lib tui::components::editor --locked
cargo test --test file_tree_trajectory --locked
cargo test --test tui_render --locked
cargo test --test tui_usability_fixes --locked
python3 scripts/check_tui_editor_text_authority.py
python3 scripts/check_tui_project_authority.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
scripts/verify.sh quick
~~~

Verify target names at implementation time, document substitutions, hosted exact-SHA CI and any unrun interactive PTY smoke. Shared CI is unsuitable for an uncalibrated fixed 5ms wall-clock assertion; establish deterministic budget measurements and record timing as diagnostics.

## 9. Acceptance and stop conditions

A. Per-frame work stays under measured explicit input/CPU/alloc bounds for any logical line length; output-only tests are insufficient.
B. Normal files retain cursor, motion, selection and Unicode behavior; pathological lines visibly degrade within limits.
C. Tree read limits *examined* entries even when all are filtered, and displays truncation.
D. Traversal cannot block async workers/UI, has cooperative cancel, and never adopts stale results.
E. Existing authorization and one-buffer static guards pass; neither pane nor dialog regresses.

Stop rather than enlarging C002 if solution needs a new remote filesystem service or whole document cache. Use smallest safe bounded fallback and record any future index/text-core contract as separate dependency.

## 10. Documentation and closure

Update `architecture/tui.md`, `docs/tui.md`, roadmap and registry. Create `plans/closure/tui-editor-correctness-usability/002-status.md` with pre/post read budgets and measured scan work, cancellation and malicious directory fixtures, test/guard/CI runs on actual SHA, compatibility, residual severity and precise close/conditional disposition.

## 11. Handoff notes

Do not equate `tokio::spawn` with `spawn_blocking`; an abort of the former does not stop an ongoing `read_dir`. Do not equate retained-node limits with number of entries examined. Keep existing two file-tree consumers attached to the same navigation→`open_editor` seam.
