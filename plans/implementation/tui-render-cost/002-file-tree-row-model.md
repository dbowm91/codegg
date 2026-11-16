# TUI Render Cost Milestone M002 — The File Tree's Row Model

Status: ready for handoff

Repository baseline: `01c45971d4330cfd1bf6541510de1eeef90b7dbc`
(branch `feat/auto-20261109-f8abad0d`)

Source roadmap:

- `plans/subsystems/tui-render-cost-roadmap.md#7-milestones` (M002)

Long-term requirements:

- `plans/000-long-term-specification.md`
- `plans/002-long-term-roadmap.md`
- `architecture/tui.md` — "The `/files` navigator: the ownership split" and
  "Depth is display arithmetic"
- `architecture/tui.md` — "The render path's caches (C006)"

Applicable ADRs: **none required.**

Primary class: **polish** (cost)

## 1. Objective

The file tree flattens and filters its listing **at most once per frame**, per
surface, behind a cache whose invalidation is proven against a stale path — so
that a removed or renamed path is never displayed, which is the reason
`repository-audit-corrective` C006 stopped short of this change.

## 2. Why this milestone is ready

No hard dependency. This milestone **discharges a named debt**:
`plans/closure/repository-audit-corrective/006-status.md` §12 records that the
file tree's row list is still rebuilt and re-cloned two to three times per
frame and that its `selected_index` lookup still rebuilds the list, and records
why it was left:

> Not touched: the milestone's budget ran out, and the file tree's rows carry
> **paths**, so a cache there needs an invalidation that is proven against a
> stale path — the plan's own stop condition.

The invalidation primitive C006 needed now exists and is already tested:
`FileTreeState` carries a `revision` that bumps only when a mutation changes
the rendered result, and a test at
`src/tui/app/state/file_tree.rs:2155` already asserts that a viewport-height
write does **not** bump it every frame. This milestone's job is to key a row
cache on that revision and prove the stale-path direction.

## 3. Current implementation evidence

Line numbers are at the baseline. Re-derive them.

### 3.1 Two surfaces, two filters, one double clone

**`FileTreeState`** (the `Ctrl+T` pane and any docked tree surface):

```rust
// src/tui/app/state/file_tree.rs:304
pub fn all_rows(&self) -> Vec<TreeRow> {
    let mut rows = Vec::new();
    visible_rows(&self.nodes, &self.expanded, &mut rows);
    rows
}

// :313
pub fn rows(&self) -> Vec<TreeRow> {
    filter_rows(&self.all_rows(), &self.filter)
}
```

`visible_rows` (`src/tui/file_tree.rs:304`) pushes a fresh `TreeRow` per visible
node, cloning `path` and `name` — two `String` allocations per row. Then
`filter_rows` (`src/tui/app/state/file_tree.rs:1025`) clones the whole vector a
second time:

```rust
pub fn filter_rows(rows: &[TreeRow], filter: &str) -> Vec<TreeRow> {
    if filter.is_empty() {
        return rows.to_vec();
    }
    let needle = filter.to_lowercase();
    rows.iter().filter(|row| matches_filter(row, &needle)).cloned().collect()
}
```

So one `rows()` call is **two full clones**, four `String` allocations per row,
plus a `to_lowercase()` of the needle. The listing is bounded at
`MAX_LISTING_NODES = 4_096` (`src/directory_listing.rs:83`), so the worst case
is ~16,000 allocations per call.

**`FilesDialog`** (the `/files` box) holds an already-flattened
`FileTreeStructure` and re-filters it on every call:

```rust
// src/tui/components/dialogs/files.rs:115
fn rows(&self) -> Vec<TreeRow> {
    filter_rows(&self.view.rows, &self.filter)
}
```

### 3.2 `/files` calls it three to four times per frame

- `render_rows` (`:208`): `let rows = self.rows();` — one call, then
  `self.selected_index_in(&rows)`.
- `reveal` (`:192`): `self.selected_index()` (which calls `rows()` again at
  `:119`) and then `self.rows().len()` — two calls.
- `title` (`:317`): `let total = self.rows().len();` — one call.

The modal path renders through `render`, which reaches `title` and
`render_rows`; the panel path renders through `render_in_panel`, which reaches
`render_rows` and `reveal`. Both are on the frame.

### 3.3 The `Ctrl+T` pane calls it once per frame, into a double clone

`src/tui/components/file_tree.rs:121` — `let rows = self.state.rows();`, on
every frame the pane is visible. The module's own comment at `:15`–`:21` claims
viewport-proportional cost; only the line loop below that call is bounded, and
the vocabulary above it is rebuilt in full.

### 3.4 The invalidation primitive already exists

`FileTreeState::revision` (`:211`) is bumped by
`FileTreeState`'s mutation paths (`:298` and the methods around it), and two
existing tests pin its discipline:

- `a_mutation_that_changes_the_rendered_result_bumps_the_revision` (`:2117`) —
  expansion and selection changes bump it;
- `a_viewport_height_write_bumps_the_revision_only_when_the_height_changed`
  (`:2155`) — a per-frame write does not.

`FilesDialog::adopt_structure` (`files.rs:106`) already returns early when the
revision it is handed is unchanged, so the flattened listing is already adopted
once per structure change. What is missing is a cache for the **filtered** rows.

### 3.5 What is *not* per-frame

`FileTreeState::rows()` also has key-handler callers (`:332`, `:351`, `:366`,
`:371`, `:460`, `:570`, `:879`). Those are per keystroke, not per frame. They
benefit from the same cache but are not the reason for this milestone; do not
enumerate them as per-frame cost.

## 4. Invariants that must not regress

- **A stale path is never displayed.** This is the milestone's load-bearing
  correctness property and the reason C006 stopped. If a directory is collapsed,
  a file is removed, the listing is replaced, or the filter changes, the next
  frame shows the new row set.
- **The rendered output is byte-identical** for a given state.
- **The row order, depth, `expanded` flag and `kind` are unchanged.** The cache
  memoizes `visible_rows` + `filter_rows`; it does not re-derive the tree.
- **The filter's semantics are unchanged**: case-insensitive, matching the file
  name as well as the path (`matches_filter`).
- **One producer of the row set.** The cache is the producer; no caller may
  re-derive rows beside it.
- **The pane's own scroll and selection behaviour is unchanged**, including the
  `reveal` clamp.

## 5. Scope

### In scope

- A filtered-row cache on `FileTreeState`, keyed on its existing `revision`
  plus the filter text.
- A filtered-row cache on `FilesDialog`, keyed on the adopted structure's
  revision plus its own filter field.
- `FilesDialog::{rows, selected_index, reveal, title, render_rows}` reduced to
  one row read per frame.
- `src/tui/components/file_tree.rs`'s per-frame `state.rows()` call.
- The `selected_index` lookup no longer rebuilding the list.
- Counters for the row builds and the filter passes.

### Explicitly out of scope

- The tree's structure, the daemon's listing, and `MAX_LISTING_NODES`.
- The `Ctrl+T` pane's visual layout and its `Depth is display arithmetic` rules.
- The filter's matching semantics.
- Any change to which rows are shown, or in what order.
- The project picker, the session dialog, or any other listing surface — they
  are separate surfaces with separate state, and bundling them would cross an
  ownership boundary.
- `/search`, per C006 §12 and this roadmap's non-goals.

## 6. Required production changes

### Core/domain

- `FileTreeState` must gain a cache holding the **filtered** rows, keyed on
  `(revision, filter)`. The accessor must hand back a borrow (`&[TreeRow]`),
  not a clone. Every existing `rows()` caller must be migrated; the ones that
  need ownership (a `.into_iter()`, an `.nth(index)`) must borrow and clone the
  one row they take.
- `all_rows()` must stop being the per-call producer: it becomes the cache's
  rebuild step, invoked only on a miss.
- `filter_rows` must gain a borrowing form for the cached path. The existing
  owned form may remain for callers that genuinely need a copy, but the render
  path must not use it.
- `FilesDialog` must cache its filtered rows keyed on
  `(self.view_revision, self.filter)`, and must read them once per render:
  `render_rows`, `title`, `reveal` and `selected_index` all take the slice.
- `selected_index_in` already takes `&[TreeRow]`; `selected_index` must stop
  calling `rows()` and must take the slice too.
- `src/tui/components/file_tree.rs` must read the cached slice.

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

None. The cache is owned by the widget, on the render thread.

### Frontend or operator surface

- Byte-identical rendering for the fixture set in §10.

### Security and authorization

The cache holds **file paths**. The implementer must:

- prove that a removed, renamed, collapsed or re-scoped path leaves the cache on
  the next frame (a test, not an argument);
- confirm the cache is per-surface and per-session, and is dropped on session
  teardown and on `set_session` / project switch, so a path from another
  workspace's listing cannot be displayed;
- confirm the cache does not extend a path's lifetime beyond the listing it came
  from — it is dropped by the same revision bump that replaces the listing.

### Documentation and static guards

- `architecture/tui.md` §"The render path's caches (C006)": one row for the file
  tree's filtered rows, keyed on `(listing revision, filter)`, dropped by either.
- `architecture/tui.md` §"The `/files` navigator: the ownership split": note
  that the box reads the adopted structure's filtered rows and does not
  re-flatten.
- `src/tui/render_cost.rs`: `file_tree_row_builds` (one per rebuild) and
  `file_tree_filter_passes` (one per filter pass). The point of the pair is that
  a frame can do a filter pass without a row build, and neither on a hit.
- `tests/tui_render/tui_render_cost.inc`: an assertion per counter.
- `scripts/check_tui_render_cost.py`: a shape check that the render path reads
  `rows()` through the cache rather than calling `filter_rows` directly. Add it
  to `--self-test`.

## 7. Ordered work packages

### WP1 — Baseline counts

Intent: a before for every claim.

Required changes: add the two counters with no behaviour change; run the new
assertions against the unmodified tree and record the failures as the baseline.

Acceptance evidence: baseline counts for a frame drawing `/files` (modal and
panel), and a frame drawing the `Ctrl+T` pane, at a listing size of ~50 rows and
at `MAX_LISTING_NODES`.

### WP2 — Cache the state's filtered rows

Intent: one producer, one borrow.

Required changes: the cache and its invalidation on `FileTreeState`; migrate
every `rows()` caller; make `all_rows` the rebuild step.

Acceptance evidence: `file_tree_row_builds` is 0 on a second frame with an
unchanged revision and filter, and 1 when either changes. A test asserts both
directions. The pane's rendered buffer is identical.

### WP3 — Prove the stale-path direction

Intent: discharge C006's stop condition, not just its cost.

Required changes: a test per invalidation source — a collapsed directory, a
removed file, a replaced listing (new revision), a changed filter, a changed
scope, and a session teardown — asserting that the next read does not contain
the stale path.

Acceptance evidence: each test fails if the cache is keyed without that input.
Demonstrate at least the removal case by perturbing the key (dropping the
revision from it) and showing the test fails; record the perturbation.

### WP4 — Reduce `/files` to one row read per frame

Intent: the three-to-four call sites.

Required changes: `render` / `render_in_panel` read the slice once and pass it
to `title`, `render_rows`, `reveal` and `selected_index`.

Acceptance evidence: `file_tree_filter_passes` is at most 1 per frame for the
modal path and at most 1 for the panel path; the box's rendered buffer is
identical, including the title's row count and the scroll clamp.

### WP5 — Cache the box's filtered rows

Intent: the box's own filter is the second key.

Required changes: the box's cache keyed on `(view_revision, filter)`.

Acceptance evidence: `file_tree_row_builds` is 0 on a second frame with an
unchanged revision and filter, and 1 when the filter field changes. A test
asserts that typing a filter character rebuilds, and that a frame with no input
does not.

### WP6 — Read the pane through the cache

Intent: remove the per-frame double clone in the `Ctrl+T` pane.

Required changes: `src/tui/components/file_tree.rs` reads the slice.

Acceptance evidence: the pane's frame does no row build on an unchanged
revision; the pane's rendered buffer is identical.

### WP7 — Prove the rendering is unchanged, and the counters bite

Intent: the primary acceptance property.

Required changes: a render comparison over a fixture set — an empty listing, a
flat listing, a nested listing with several directories expanded, a truncated
listing (`truncated` set), a filtered listing, a filtered listing with no
matches, and a listing with a non-ASCII name. Then perturb the cache key and
confirm the comparison fails.

Acceptance evidence: identical buffers; a failing comparison under the
perturbation; the perturbation reverted and recorded.

## 8. Failure, cancellation, restart, and contention semantics

- **Failure.** A cache that survives a structure change displays a path that is
  gone. This is the milestone's failure mode and its stop condition. There is
  no partial success here: the invalidation is either proven or the milestone
  does not land.
- **Cancellation.** A session teardown and a project switch must drop the cache.
- **Restart.** Nothing durable; the cache starts empty.
- **Contention.** None; one widget, one thread.
- **A listing that arrives mid-frame.** `refresh_file_tree_surfaces` already
  pushes the structure into mounted surfaces before any of them draws, and
  `adopt_structure` skips on an unchanged revision. The cache must be dropped by
  the same revision that `adopt_structure` observes, so a structure adopted on
  this frame is drawn on this frame.

## 9. Compatibility and migration

None. No protocol, storage, configuration, or migration change. The change is
internal to `src/tui/`. `FileTreeState::rows()`'s signature changes from
returning `Vec<TreeRow>` to borrowing; that is a `pub` API within the crate and
every caller is migrated in the same change.

## 10. Required tests

### Focused unit tests

- A second read with an unchanged revision and filter performs no row build.
- An expansion change rebuilds.
- A selection change rebuilds (the selection is rendered, so it is part of the
  revision — see the existing test at `:2117`).
- A viewport-height write does not rebuild (the existing test at `:2155` must
  keep passing).
- A filter change rebuilds.
- `FilesDialog::title`, `render_rows`, `reveal` and `selected_index` all read
  one slice.
- `selected_index` does not rebuild the list.

### Integration tests

- A frame drawing `/files` and a frame drawing the `Ctrl+T` pane assert the
  expected counts for both counters, driven through `tests/tui_render`.

### Restart and recovery tests

None required.

### Contention and cancellation tests

- A session teardown and a project switch drop the cache (the next read
  rebuilds).

### Security and negative tests

- A removed path is absent from the next read.
- A renamed path shows the new name.
- A collapsed directory's children are absent.
- A path from a previous scope is absent after a scope change.

### Migration and compatibility tests

None.

### Placement and vacuity

`tests/tui_render/tui_render_cost.inc` for the counts; the state's own unit
tests (`src/tui/app/state/file_tree.rs`) for the invalidation matrix, beside the
two tests that already pin the revision discipline. Every new assertion must be
shown failing against the baseline.

## 11. Required verification commands

```bash
# The state's own tests, including the two existing revision-discipline tests.
cargo test -p codegg --lib -- tui::app::state::file_tree

# The file tree's component tests.
cargo test -p codegg --lib -- tui::components::file_tree \
    tui::components::dialogs::files

# The cost member and the family.
cargo test --test tui_render -- tui_render_cost
cargo test --test tui_render

# The shape guard, and the proof it bites.
python3 scripts/check_tui_render_cost.py
python3 scripts/check_tui_render_cost.py --self-test

# Rust-aware test inventory.
python3 scripts/rust_test_scan.py --validate

# Scoped, then the full sweep before closure.
scripts/verify.sh scoped HEAD~1
bash scripts/verify.sh full
```

## 12. Documentation updates

- `architecture/tui.md` §"The render path's caches (C006)": the new row.
- `architecture/tui.md` §"The `/files` navigator: the ownership split": the box
  reads filtered rows from the adopted structure; it does not re-flatten.
- `src/tui/components/file_tree.rs`'s module comment at `:15`–`:21` must be
  corrected: it currently claims viewport-proportional cost while the
  vocabulary above the loop is rebuilt. After this milestone the claim is true;
  until then it is not. Either way the comment must match the code.

## 13. Acceptance criteria

1. A frame that draws `/files` performs **at most one** filter pass and **at
   most one** row build, down from three to four filter passes and two clones
   each.
2. A frame that draws the `Ctrl+T` pane performs **zero** row builds when the
   revision and filter are unchanged, and **one** when either changes.
3. `selected_index` does not rebuild the row list.
4. A removed, renamed, collapsed, re-scoped or replaced path is never present in
   the next read — asserted per source, with at least one perturbation shown
   failing.
5. The rendered output is byte-identical to the baseline for the fixture set,
   and the comparison is demonstrated capable of failing.
6. The listing's bound (`MAX_LISTING_NODES`), the filter's semantics, the row
   order and the `reveal` clamp are unchanged.

## 14. Stop conditions

Stop and report rather than improvise if:

- The invalidation cannot be proven for any source in §10's negative tests. A
  stale path displayed to the user is a correctness defect and this milestone
  does not land with one.
- The cache would have to be shared across the two surfaces (`FileTreeState` and
  `FilesDialog`) to meet the criteria. They have separate filters and separate
  revisions; a shared cache would be a second ownership boundary.
- Borrowing the rows would require changing `render_rows`' or `reveal`'s
  signatures in a way that reaches outside `src/tui/components/dialogs/files.rs`
  and `src/tui/components/file_tree.rs`.
- The change would alter which rows are shown, their order, or the filter's
  matching.
- The change would touch the project picker, the session dialog, or any listing
  surface outside the two named here.
- A stale-path test cannot be made to fail under a perturbed key — that would
  mean the test is not testing the invalidation.

## 15. Closure evidence required

- The baseline and after counts for both counters, per surface, with the
  listing sizes used.
- The invalidation matrix's results, and the perturbation that was shown
  failing.
- The render comparison's result and its demonstrated ability to detect a
  difference.
- The confirmation that no path outlives its listing or its session, with the
  test that proves it.
- The corrected module comment.
- `cargo test --test tui_render`, the state's and components' unit tests,
  `scripts/verify.sh quick`; `scoped` and `full` labelled local versus CI.
- The registry update, and the unblock audit. This milestone is expected to
  unblock nothing, and to **discharge** the file-tree finding C006 §12
  carried over — record that discharge explicitly, because it is the reason the
  finding can leave the registry's carried-over list.

## 16. Handoff notes

- **The stale-path proof is the milestone, not the speedup.** C006 already
  measured the cost and stopped for this reason. Land the invalidation matrix
  before the cache's fast path, so a wrong key fails loudly.
- **`rows()` is two clones, not one.** `all_rows()` clones per node and
  `filter_rows` clones the vector. Fixing only one of them halves nothing
  meaningful.
- **`revision` already exists and is already tested.** Do not invent a second
  revision. The two tests at `:2117` and `:2155` are the contract; keep them
  green.
- **The box and the pane are different surfaces with different filters.**
  `FilesDialog` filters an adopted `FileTreeStructure`; `FileTreeState` filters
  its own tree. Two caches, two keys.
- **`reveal` calls `rows()` twice today** — once via `selected_index`, once for
  the length. Both are the same slice.
- **Do not touch `/search`.** It is out of scope by roadmap §3 and by C006 §12.
- **Correct the module comment either way.** A comment that claims a bound the
  code does not have is the defect this milestone was created to remove.
