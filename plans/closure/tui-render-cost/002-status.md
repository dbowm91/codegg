# TUI Render Cost Milestone 002 — Closure Status

Status: **closed** (2026-11-10). This milestone **discharges** the file-tree
finding that `plans/closure/repository-audit-corrective/006-status.md` §12
carried over.

Implementation plan:
`plans/implementation/tui-render-cost/002-file-tree-row-model.md`
Source roadmap: `plans/subsystems/tui-render-cost-roadmap.md` §7 (M002)
Repository baseline: `01c45971d4330cfd1bf6541510de1eeef90b7dbc`
No ADR was required, and none was created.

## 1. Executive finding

The file tree flattened and filtered its listing on every read. `FileTreeState`
built the whole vocabulary per call (`visible_rows`, two `String` allocations per
row) and then cloned it again in `filter_rows` — four allocations per row, with
the listing bounded at `MAX_LISTING_NODES = 4_096`, so a single `rows()` call
could allocate ~16,000 times. `/files` called it three to four times per frame
(`render_rows`, `reveal` twice through `selected_index` and its own length, and
`title`), and the `Ctrl+T` pane called it once per frame straight into that double
clone.

C006 measured this and stopped, because the rows carry **paths**: a cache there
needs an invalidation proven against a stale path. That proof is what this
milestone adds, and it is its first acceptance criterion rather than its speedup.

## 2. The cost sites, before and after

Operation counts on one **unchanged** frame (the second frame after a warm-up),
measured through `file_tree_row_builds` and `file_tree_filter_passes`.

| Surface | ~50 rows | `MAX_LISTING_NODES` |
|---|---|---|
| `Ctrl+T` pane | 1 build, 1 pass → **0, 0** | 1, 1 → **0, 0** |
| `/files` modal | 0 builds, 4 passes → **0, 0** | 0, 4 → **0, 0** |
| `/files` docked panel | 0 builds, 3 passes → **0, 0** | 0, 3 → **0, 0** |

`selected_index` no longer rebuilds the row list; `reveal`, `title` and
`render_rows` read one borrowed slice.

### 2.1 The captured baseline failures

```
the_tree_pane_rebuilds_nothing_on_an_unchanged_frame ... left: 1, right: 0
  "the pane flattened the whole listing again on a frame where nothing changed"
a_files_frame_that_changed_nothing_builds_and_filters_nothing ... left: 4, right: 0
  "a `/files` frame that changed nothing filtered the row list"
a_files_frame_after_a_listing_arrival_filters_at_most_once ... left: 1, right: 0
  "the box re-flattened a structure the authority had already built"
a_pane_filter_change_rebuilds_once_and_the_next_frame_reads_it ... left: 0, right: 1
a_listing_arrival_builds_the_rows_once_and_the_next_frame_reads_them ... left: 1, right: 0
a_listing_at_the_bound_is_not_rebuilt_per_frame ... left: 1, right: 0
```

## 3. The shape

- `FileTreeState` gained a private `RowCache` holding the unfiltered and the
  filtered rows, keyed on `(revision, filter)`. `bump()` is the **only** rebuild
  point, and it is already called by every mutation that changes the rendered
  result — including `filter_push`/`filter_pop`. `rows()` and `all_rows()` return
  `&[TreeRow]`, so the render path takes one borrow and allocates nothing.
- `FilesDialog` gained its own `RowsCache` keyed on `(view_revision, filter)`,
  rebuilt at exactly its three writes: `adopt_structure`, and the two filter
  edits. `render`/`render_in_panel` read one borrow and pass it to
  `title_in`, `render_rows` and `selected_index`.
- `structure()` now clones the cached unfiltered rows instead of re-flattening,
  and is still gated on `revision` by both callers. It is deliberately **not**
  counted as a row build: the counter counts flattens.

## 4. The invalidation matrix — the milestone's load-bearing property

| Source | Test | Normal | Under the perturbation |
|---|---|---|---|
| removed | `a_removed_path_is_absent_from_the_next_read`, `a_removed_path_never_reaches_a_drawn_frame` | pass | **fail** |
| renamed | `a_renamed_path_is_read_under_its_new_name`, `a_renamed_path_is_drawn_under_its_new_name` | pass | **fail** |
| collapsed | `a_collapsed_directory_hides_its_children_on_the_next_read` | pass | **fail** |
| re-scoped | `a_path_from_the_previous_scope_is_absent_after_a_scope_change` | pass | **fail** |
| replaced listing | `a_replaced_listing_replaces_every_row` | pass | **fail** |
| filter change | `a_filter_change_replaces_the_rows_it_shows` | pass | pass (the filter half is still keyed) |
| project switch | `a_project_switch_drops_the_rows` | pass | **fail** |
| session teardown | `two_states_never_share_a_row_list` | pass | **fail** |

**The perturbation, recorded and reverted.** Dropping the revision from the cache
key (`row_cache.revision == self.revision && row_cache.filter == self.filter` →
`row_cache.filter == self.filter`) made **38 of 62** state tests fail — all six
stale-path tests plus the count tests — and **5 of 12** family tests, including
both drawn-text stale-path tests. That is the proof the comparison can detect the
defect the milestone exists to prevent, and it is the reason the finding can leave
the carried-over list.

## 5. Invariant review

- **A stale path is never displayed.** Proved per source above.
- **The rendered output is byte-identical.** Warm/cold buffer comparison over the
  fixture set: empty listing, flat listing, nested listing with several
  directories expanded, truncated listing, filtered listing, filtered-with-no-
  matches, and a non-ASCII name.
- **Row order, depth, `expanded` and `kind` are unchanged.** The cache memoizes
  `visible_rows` + `filter_rows`; it does not re-derive the tree.
- **The filter's semantics are unchanged** (case-insensitive, matching the name
  as well as the path).
- **One producer of the row set.** `bump()` is the only rebuild point; the guard
  pins the key's two halves and forbids both surfaces from flattening or
  filtering while drawing.
- **The pane's scroll and selection behaviour is unchanged**, including the
  `reveal` clamp.

## 6. Security review

The cache holds **file paths**, so it was reviewed as a lifetime question:

- a removed, renamed, collapsed or re-scoped path leaves the cache on the next
  read — a test per source, not an argument;
- the cache is per-surface (two caches, two revisions, two filters) and per-
  session: it is a field of the state, rebuilt by the same bump that replaces the
  listing, and dropped with the `App`, so a path from another workspace's listing
  cannot be displayed;
- it introduces no lifetime the tree did not already have.

No authorization boundary change.

## 7. Migration and compatibility review

`FileTreeState::rows()` and `all_rows()` changed from returning `Vec<TreeRow>` to
borrowing `&[TreeRow]`. Every caller was migrated in the same change: ~22 in
`state/file_tree.rs` (several in tests), `components/file_tree.rs`, and the
`FilesDialog` internals. A caller that needs ownership borrows and clones the one
row it takes.

Two API consequences, both recorded rather than hidden:

- **`RowCache` and `row_cache` are `pub(crate)`.** Functional-record-update from
  another module needs the field visible, and a `pub(crate)` field may not name a
  module-private type under `-D warnings`. The consequence is that **FRU from an
  integration test cannot compile**; `tests/tui_render/file_tree_trajectory.inc`
  was rewritten to `FileTreeState::default()` plus field writes for that reason.
  A future integration-test FRU will break.
- **`FilesDialog::title` is now `#[cfg(test)]`**; the frame calls `title_in(rows)`
  so the title's count and the drawn rows come from one borrow.

Three existing tests were rewritten to write through the authority's verbs
(`expand_and_select`, `collapse`, `filter_push`) instead of the public fields.
Every assertion is unchanged — the rewrites are the invariant this milestone
introduces, not a weakening.

## 8. Verification executed

```text
$ cargo fmt --check
exit=0
$ cargo test -p codegg --lib -- tui::app::state::file_tree
test result: ok. 62 passed; 0 failed; 0 ignored; 0 measured; 7783 filtered out
$ cargo test -p codegg --lib -- tui::components::file_tree tui::components::dialogs::files
test result: ok. 40 passed; 0 failed; 0 ignored; 0 measured; 7805 filtered out
$ cargo test --test tui_render -- tui_file_tree_cost
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 1126 filtered out
$ cargo test --test tui_render
test result: ok. 1138 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
$ python3 scripts/check_tui_render_cost.py
check_tui_render_cost: the render path derives each result once
$ python3 scripts/check_tui_render_cost.py --self-test
self-test: 14 injections, all rejected
$ python3 scripts/rust_test_scan.py --validate
rust_test_scan --validate: all checks pass
```

The family count is the count at this milestone's closure. The tree-level
`scripts/verify.sh quick` result is recorded once, in the roadmap's closure.

## 9. Unresolved findings and deferrals

- **A listing arrival costs 3 builds and 3 filter passes**, not the 1 the plan's
  wording implies: `begin_rebuild`'s bump rebuilds the old nodes, `apply_listing`
  bumps after the `expanded.retain` and rebuilds from the new listing, and the
  final bump rebuilds after the selection/reveal fix-up. With a pending cursor,
  `enter_directory` adds a fourth (reasoned from the code, not separately
  measured). Reducing it needs a second revision for row inputs, which the plan
  forbids; the load-bearing per-frame criterion — 0 builds on an unchanged frame —
  holds. Reported rather than fudged.
- **The `pub` row inputs are now guard-enforced** (corrective pass,
  2026-11-10). `nodes`, `expanded` and `filter` stay `pub`, but
  `check_tui_render_cost.py`'s `file-tree-row-input-write` check rejects any
  write to them through `file_tree_state.` from outside the authority's own
  module — an assignment or an in-place mutator, with the authority's own verbs
  (`filter_push`, `filter_pop`, `expand_and_select`, `collapse`) exempt because
  they carry no dot. It walks `src/tui` and `tests/tui_render` rather than a
  fixed file list, so a new file is covered too, and its `--self-test` injection
  writes `file_tree_state.expanded.insert(..)` from `src/tui/app/modal.rs`.
- **`expand_and_enter` had the same defect shape as `apply_listing`** — it
  inserted into `expanded` and then read rows before its bump, so the cursor
  landed on the directory instead of its first child. The cache exposed it; an
  existing test caught it; it is fixed and an expand-and-enter now honestly
  builds twice.
- **The plan's own §12 note about the pane's module comment** is discharged: the
  comment at the top of `src/tui/components/file_tree.rs` now says the vocabulary
  is built once per revision and the widget borrows the slice.

## 10. Roadmap disposition and registry update

M002 is **closed** and **discharges** the carried-over finding: the file tree's
row list is no longer rebuilt and re-cloned two to three times per frame, and its
`selected_index` lookup no longer rebuilds the list. The finding can leave
`plans/closure/repository-audit-corrective/006-status.md` §12's list.

The roadmap's milestone table records M002 as `closed` with this record linked;
`plans/registry.md` moves its row to the closed disposition. The unblock audit
found nothing to unblock.
