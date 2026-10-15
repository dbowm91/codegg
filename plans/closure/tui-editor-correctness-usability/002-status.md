# TUI Editor Correctness and Usability C002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-editor-correctness-usability/002-bounded-editor-render-and-tree-walk.md`

Source subsystem roadmap:

- `plans/subsystems/tui-editor-correctness-usability-roadmap.md#C002`

Repository baseline reviewed: `0c11423cc0d7a1bbc0258a23832240bd0381c55f`
(C001 closure; branch `plans/tui-editor-correctness-usability-20261013`).

Implementation commits or pull requests:

- `bd891348f` — tui-editor(c002): bounded render budgets and cancellable tree walks

## 1. Executive finding

C002 is complete as a corrective frontend slice. Per-frame editor work is
now bounded in input bytes (32 KiB per visible line, 256 KiB per frame),
not output rows: 1–4 MiB single-line documents render a visibly marked
prefix with cursor/selection bytes intact, at cursor start, middle, and far
right, on 80x24 and 1x1 terminals. The tree walk counts examined entries
independently of retained nodes (100,000 vs 4096), runs blocking I/O on
`spawn_blocking` with a one-live-task limit and cooperative cancellation,
never walks synchronously on the event loop, and fails closed on
non-local roots. `/files`, the Ctrl+T pane, and editor attachment share the
one guarded rebuild seam. No protocol, storage, migration, schema, or
authorization change. Historical M006 closures are preserved; this is new
corrective evidence only.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Per-frame ≤256 KiB source bytes, ≤32 KiB per visible line, measured deterministically (no wall-clock assertion) | `frame_cost_stays_within_budget_for_a_4mib_single_line` (total exactly 32 KiB, max-line 32 KiB, 1 truncated); `frame_cost_stays_within_budget_for_many_huge_lines` (30×1 MiB lines total exactly 256 KiB, 30 truncated); `measure_frame_cost` mirrors `render_text` exactly | pass | Budgets are `MAX_RENDER_LINE_BYTES` / `MAX_FRAME_SOURCE_BYTES`; `FrameCost{total_bytes,max_line_bytes,truncated_lines,column_scans}` |
| Extreme line at left/right/middle | `a_1mib_single_line_renders_a_marked_prefix_within_budget` (cursor 0); `cursor_at_the_far_right_of_a_huge_line_stays_bounded_and_marked` (cursor at end); `cursor_in_the_middle_of_a_huge_line_stays_bounded_and_marked` | pass | All assert `too long` marker + frame budget |
| Column seeking with tabs/Unicode | `bounded_prefix_matches_exact_for_short_lines_tabs_and_unicode` (bounded == exact incl. tabs, CJK width 2, combining width 0); `tabs_and_wide_unicode_inside_the_budget_render_exactly`; `bounded_prefix_end_never_splits_a_multibyte_char` | pass | Window edge lands on a char boundary; short lines byte-identical to before |
| Tiny terminal | `a_1x1_terminal_with_a_huge_line_does_not_panic_and_marks` + pre-existing `a_narrow_area_does_not_panic` | pass | 1x1, 1x10, 10x1, 2x2, 3x3 |
| Line-state warning, never silent truncation | Row suffix `…[too long]` (warning bold) in `line_spans` + frame-exhausted rows; banner `line too long — showing prefix` when cursor line/column past window | pass | Suffix + banner asserted via joined render text |
| Normal-file golden frame unchanged | Pre-existing `renders_line_numbers_and_the_line_content`, `a_very_long_line_is_truncated_to_the_viewport` (5000 chars < budget, exact path), `tui_render` 110/110, `tui_usability_fixes` 18/18 | pass | Short-line paths are the same code as before, budget-gated only above 32 KiB |
| Tree examines *examined* entries even when all filtered | `examined_budget_stops_a_walk_of_only_filtered_entries` (200 symlinks, cap 50 → truncated with `examining` reason, 0 nodes) | pass | Examined increments before any filter; production cap 100,000 |
| Accepted-node cap distinct from examined cap | `node_cap_and_examined_cap_are_independent_reasons` (node reason names `entries`, never `examining`); pre-existing `node_cap_truncates_and_says_so` | pass | Either budget can fire; reason says which |
| Large ignored/symlink directory metadata | `examined_counts_ignored_and_special_entries_without_retaining_them` (examined == 2, ignored counted); `symlink_flood_counts_examined_without_truncation_or_retention` (1001 symlinks examined, 1 retained, no truncation) | pass | `TreeListing.examined` is the result metadata |
| Unreadable subdir | Pre-existing `unreadable_directory_does_not_blank_the_tree` passes | pass | `read_dir` failure still non-fatal per directory |
| Traversal loop | Pre-existing `symlinks_are_never_followed_at_any_depth`, `symlink_loop_terminates` pass | pass | Symlink check stays on the raw entry |
| Async cancellation (superseded scan finishes within checkpoint budget) | `a_cancelled_walk_reports_cancellation_not_a_partial_tree`; `cancel_mid_walk_stops_a_deep_tree`; state `a_rebuild_cooperatively_cancels_the_previous_walk`, `reset_for_project_cancels_an_in_flight_walk`; `prepare_shutdown` calls `cancel_in_flight` | pass | Flag checked on every entry + before each descent; cancellation reports truncation, never a partial tree as complete |
| 100 rapid refreshes without unbounded workers | `a_hundred_rapid_rebuilds_keep_one_live_task` (100 rebuilds → exactly 1 live `FileTree` task, generation +100); state `a_hundred_rapid_rebuilds_leave_one_generation_and_no_stranded_flag` (99 superseded flags set, latest live) | pass | `cancel_kind(FileTree)` before each spawn = hard limit one |
| Project switch race | `a_completion_from_before_a_project_switch_is_refused` (walk for root A completes, switch to B + rebuild, A refused and never renders); pre-existing stale-generation/root tests | pass | Generation + root + request triple gate at `apply_listing` |
| Remote-root fail closed | `local_walk_root_refuses_non_local_roots` (missing, relative, file-as-root, symlinked root refused; real dir accepted); `rebuild_refuses_a_non_local_root_instead_of_walking_it` (no spawn, explicit `refusing` error) | pass | Refusal point documents remote listing unsupported; no new listing protocol |
| No synchronous walk in the missing-channel fallback | `rebuild_without_a_command_channel_fails_explicitly_without_walking` (explicit `command channel` error, zero nodes) | pass | Old inline `build_tree` fallback removed |
| Blocking I/O off Tokio workers | `rebuild` runs `build_tree_with_cancel` inside `spawn_blocking` within the scoped task; abort strands at most one more entry via the flag | pass | Structural; `tokio::spawn` ≠ `spawn_blocking` confusion removed |
| Existing authorization guards | `check_tui_editor_text_authority.py` pass; `check_tui_project_authority.py` pass | pass | No second buffer, no renderer I/O, no cwd authority |

Before/after fixtures: pre-fix `line_slice` did
`read_bytes(start, end)` over the whole logical line (a 4 MiB line
allocated ~4 MiB per visible row per frame), `line_content_end` read the
whole line to trim one terminator, and `longest_visible_width` measured
complete visible lines — a single enormous line dominated the frame while
`per_frame_work_is_bounded_by_the_viewport_not_the_document` asserted only
row counts. Pre-fix `walk` decremented the node budget only for accepted
entries, so millions of ignored/symlink entries scanned unboundedly, and
`rebuild` ran blocking `read_dir` work in `tokio::spawn` (abort cannot
preempt it) with a synchronous no-channel fallback. Post-fix the same
shapes cost 32 KiB / 256 KiB exactly (asserted above), ignored entries
consume the independent examined budget, and every walk is blocking-isolated,
cooperatively cancellable, and generation-guarded.

## 3. Production implementation evidence

- `src/tui/editor.rs`: `MAX_RENDER_LINE_BYTES` (32 KiB),
  `MAX_FRAME_SOURCE_BYTES` (256 KiB), `TRUNCATED_LINE_SUFFIX`.
  `line_byte_len`/`line_is_truncated` are length-only (no content read).
  `bounded_prefix_end` clips a window to a char boundary. `line_content_end`
  inspects at most the last two bytes. `bounded_display_width`,
  `display_column` (saturating) + `display_column_truncated`,
  `offset_at_column` (clamps to prefix end), `longest_visible_width` (prefix
  windows + 256 KiB frame stop), `frame_truncated_lines`, and
  `measure_frame_cost`/`FrameCost` mirroring `render_text`. `resolve_viewport`
  uses the bounded cursor column; cursor bytes untouched.
- `src/tui/components/editor.rs`: module docs state the budgets;
  `line_slice` reads the prefix window only; `line_spans` renders huge lines
  as plain prefix + warning `…[too long]` (no per-token mapping off-window);
  `render_text` enforces the 256 KiB frame total (over-budget rows are
  marker-only, zero bytes read); banner adds `line too long — showing
  prefix` for cursor line/column past the window.
- `src/tui/file_tree.rs`: `MAX_TREE_EXAMINED` (100,000) counted before any
  filter; `TreeListing.examined` metadata; `build_tree_with_cancel`
  (flag checked per entry + per descent, cancellation reports truncation);
  `local_walk_root` fail-closed gate (absolute + dir + non-symlink +
  readable, else explicit refusal).
- `src/tui/app/state/file_tree.rs`: `cancel_flag` lifecycle —
  `begin_rebuild` supersedes (sets old flag, mints new),
  `take_cancel_flag` moves it into the blocking task, `cancel_in_flight`
  (idempotent), `reset_for_project` cancels.
- `src/tui/commands/file_tree.rs`: `rebuild` resolves authority from the
  active tab only, refuses non-local roots, aborts previous `FileTree` tasks
  (hard limit one), errors explicitly without a channel (no sync walk), and
  runs the scan on `spawn_blocking` with the cooperative flag; stale
  completions still refused by generation/root/request in `apply_listing`.
  `/files` refresh, Ctrl+T pane, and editor refresh share this seam.
- `src/tui/app/mod.rs`: `prepare_shutdown` signals the in-flight walk flag
  before `cancel_all`.
- Docs: `architecture/tui.md` C002 subsection; `docs/tui.md` long-line /
  file-tree subsection.

Ownership preserved: snapshot remains the only text source; no
presentation text cache (bounded revision-keyed checkpoints were not
needed); tree stays navigation-only with `open_editor` the sole
document-opening path; symlinks never followed.

## 4. Verification executed

### Commands run

```bash
cargo test --lib --locked -- tui::editor tui::file_tree
cargo test --lib --locked -- tui::components::editor tui::app::state::file_tree tui::commands::file_tree tui::components::file_tree tui::components::dialogs::files
cargo test --locked --test file_tree_trajectory
cargo test --locked --test tui_render
cargo test --locked --test tui_usability_fixes
cargo test --lib --locked -- tui::
python3 scripts/check_tui_editor_text_authority.py
python3 scripts/check_tui_project_authority.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
scripts/verify.sh quick
```

All suites exist under the plan's names; no substitutes were needed.

### Results

- `tui::editor` + `tui::file_tree` filter: 82 passed / 0 failed (includes
  8 new editor-budget tests and 7 new examined/cancel/locality tests).
- components/state/commands file-tree + editor filter: 69 passed / 0 failed
  (includes 6 new render-degradation tests, 3 new cancel-flag tests, 4 new
  command tests).
- Broad `tui::`: 1476 passed / 0 failed (1448 at C001 close + 28 new).
- `file_tree_trajectory`: 4 passed / 0 failed (navigator→`open_editor`
  properties intact).
- `tui_render`: 110 passed / 0 failed.
- `tui_usability_fixes`: 18 passed / 0 failed.
- `check_tui_editor_text_authority.py`: pass (one borrowed-snapshot note).
- `check_tui_project_authority.py`: pass.
- `cargo fmt --check`: pass.
- `cargo clippy --workspace --all-targets -- -D warnings`: pass.
- `scripts/verify.sh quick`: pass (fmt, agent schema, core/client/desktop
  boundaries, sandbox, execution-ownership, TUI authority/project authority,
  http-route-disposition, audit, scheduler-bypass, config-merge, provider
  wire/catalog/resilience, OpenAI endpoint, Eggwork routing, workspace check).

Local truth only. Hosted root CI and Desktop E2E were not run from this
environment (see §10 low finding). All renderer/traversal tests above ran
locally on the implementation head.

## 5. Invariant review

- No second document buffer: guard green; budgets are read windows, not
  caches; widget still borrows the snapshot per frame.
- No direct editor filesystem read: guard green; tree reads directory
  entries only, never file bytes; opening stays `open_editor`-only.
- No project authority from cwd: guard green; rebuild resolves the active
  tab; locality refusal never consults process cwd.
- Symlink following: never; root-symlink refused, entry symlinks skipped on
  the raw path.
- No silent truncation: row suffix + banner note + truncation reasons; empty
  and error states distinct (`Empty workspace` vs `Scanning…` vs error
  line).
- No unbounded blocking worker growth: one live task + cooperative flag +
  examined cap bound every scan even after abort.
- No stale adoption: generation/root/request at completion; superseded
  flags set before the new walk spawns.

## 6. Failure and recovery review

- Scan cancelled/superseded: no authoritative listing; older root never
  replaces a newer tab's view (switch test).
- Blocking task after async abort: exits within one entry via the flag;
  total work still capped at 100,000 examined entries.
- Repeated refreshes: exactly one live task; 99 superseded flags all set.
- Line-budget exhaustion: editor state intact, warning shown, drafts
  untouched, no disk reads outside the controller.
- Shutdown: flag set + `cancel_all`; scans drain within the checkpoint
  budget.
- Missing channel / non-local root: explicit `Error` status with a reason,
  never a stuck `Loading` or a same-name local walk.

## 7. Migration and compatibility review

No schema, protocol, persisted config, DTO, or ordering change. Local
file-tree ordering, symlink handling, names, and normal editor output
preserved (trajectory + render suites green). Huge-line motion limited to
the rendered prefix is described in `docs/tui.md` with normal-file behavior
unchanged. M006 historical closures immutable; this record is new
corrective evidence.

## 8. Security review

No path/document authority broadened. `check_tui_editor_text_authority.py`
green. `check_tui_project_authority.py` green. Allocation DoS bounded by
32 KiB / 256 KiB windows and the 100,000-entry walk cap. No secrets logged
(performance counters count bytes/entries, never contents). No new
authorization descriptor; tree remains navigation-only.

## 9. Documentation and operations

- `architecture/tui.md`: C002 subsection (budgets, `FrameCost` mirror rule,
  truncation signals, examined vs node budgets, blocking/cancel/concurrency
  design, refusal point).
- `docs/tui.md`: user-facing long-line + file-tree subsection (prefix
  behavior, motion limit, limits and staleness signals, remote unsupported).
- Operational signals: `…[too long]` row marker, `line too long — showing
  prefix` banner, tree truncation reasons naming the fired budget,
  `Scanning workspace…` vs error lines, one-live-task registry bound.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Hosted root CI and Desktop E2E not run on `bd891348f` | Strict closure prefers exact-SHA hosted green; local `verify.sh quick` + 1476 TUI tests + clippy green substitute locally | Run hosted CI on the merge head; record runs here by factual correction if they fail |
| low | No live PTY/interactive TUI smoke | Roadmap calls one desirable; absence recorded, not claimed | Optional live smoke on a future pass |
| low | Examined-cap branch at the production 100,000 threshold not exercised with 100,000 real files | The committed tests prove the mechanism (filtered-only walk stops with the `examining` reason at a small budget; 1001-symlink flood counts exactly) through the identical code path; creating 100k files is unsuitable for a unit test | A >100k ignored-entry fixture remains a manual work-package-A demonstration, not a committed test |

No critical/high/medium findings. No corrective pass required.

## 11. Roadmap disposition

Milestone C002 closed; C001 stays closed; M003 remains blocked on proposed
ADR-0021; M004 remains blocked on M003 (C001 and C002 now both satisfied,
M003 still pending, so M004 stays blocked). No scope expansion into
completion or remote filesystem service. No registered downstream plan was
unblocked by this closure.

## 12. Registry updates

- `plans/implementation/tui-editor-correctness-usability/002-bounded-editor-render-and-tree-walk.md`:
  `ready for handoff` → `implemented; closed`.
- `plans/subsystems/tui-editor-correctness-usability-roadmap.md` C002 row:
  `ready` → `closed`; closure linked.
- `plans/registry.md`: C002 `ready` → `closed` with closure
  `plans/closure/tui-editor-correctness-usability/002-status.md` and
  implementation `bd891348f`; unblock audit recorded (M003 still blocked on
  ADR-0021, M004 still blocked on M003; nothing unblocked).
