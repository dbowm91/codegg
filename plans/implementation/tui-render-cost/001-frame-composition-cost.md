# TUI Render Cost Milestone M001 — The Frame's Own Per-Frame Work

Status: ready for handoff

Repository baseline: `01c45971d4330cfd1bf6541510de1eeef90b7dbc`
(branch `feat/auto-20261109-f8abad0d`)

Source roadmap:

- `plans/subsystems/tui-render-cost-roadmap.md#7-milestones` (M001)

Long-term requirements:

- `plans/000-long-term-specification.md`
- `plans/002-long-term-roadmap.md`
- `architecture/tui.md` — "The render path's caches (C006)" and the frame
  contracts in "Invariants & Gotchas"

Applicable ADRs: **none required.** No identity, ownership, protocol, storage,
or authorization change. Every change is a cost change with an unchanged
rendered output.

Primary class: **polish** (cost)

## 1. Objective

A drawn frame stops paying for work it does not use: it no longer deep-clones
the terminal buffer unless a selection is live, no longer re-materializes the
sidebar's projection and line model more than once, no longer clones an open
subagent tab, no longer builds the composer hint line twice, and no longer
builds the tasks overlay's rows twice.

## 2. Why this milestone is ready

No hard dependency. Every file this milestone touches is stable at the
baseline, and the milestone consumes no other milestone's output. The
`tui-render-cost` roadmap classifies all six milestones as mutually
independent (§6), and M001 is sequenced first by consequence, not by
dependency: it is the largest unconditional per-frame cost and the smallest
change.

Its interface dependencies are read-only: `architecture/tui.md` owns the frame
cadence and the `catch_unwind` boundary, and this milestone does not touch
either.

## 3. Current implementation evidence

All line numbers are at the baseline above. Re-derive them; do not trust them
blindly.

### 3.1 The retained frame buffer is cloned on every frame for a reader that returns early

`src/tui/app/render.rs`, at the end of `finish_render`:

```rust
if let (Some(anchor), Some(focus)) = (self.selection_anchor, self.selection_focus) {
    let selection = crate::tui::selection::TextSelection::new(anchor, focus);
    ...
    self.last_frame_buffer = Some(buffer.clone());
} else {
    self.last_frame_buffer = Some(frame.buffer_mut().clone());
}
```

`last_frame_buffer` has exactly **one** reader in the whole tree:
`App::copy_selection_to_clipboard` (`src/tui/app/mod.rs:9070`), which begins
with `let (Some(anchor), Some(focus)) = (self.selection_anchor, self.selection_focus) else { return; }`.

So the `else` arm's clone is dead work on every frame: `width × height`
cells, each a `Cell` holding a `CompactString` symbol, a foreground, a
background, an underline colour and a modifier set. On a 200×60 terminal that
is 12,000 cell copies per frame — roughly 750,000 per second at the streaming
cadence.

No test in `tests/` references `last_frame_buffer`.

### 3.2 The sidebar's projection is re-materialized per frame, per occupied panel

`src/tui/app/render.rs:2785` — `render_panel_at` begins with
`self.populate_session_panel();`, and `render_panel_at` is called from the loop
over `regions.occupied()` (`:402`). So the population step runs **once per
occupied panel, every frame** — including when the occupied panel is not the
Session panel.

`populate_session_panel` (`:2817`–`:3030`) rebuilds, by deep clone:

- the project scope, session, agent name, model and provider;
- `session_state.mcp_servers.clone()`;
- every changed file as a fresh `SidebarFileChange`, each carrying
  `path.to_string_lossy().into_owned()`, `action.clone()`,
  `diff_preview.clone()` (a `Vec<String>`) and `diff_state.clone()`;
- the whole `GitSidebarInfo`: root, branch, error, operation-state label,
  available actions, conflicted paths;
- `derived.goal.clone()` and `derived.plan.clone()`;
- from the projection snapshot: every tool program, every agent run (nine
  cloned fields each), every run group, the agent tree, every convergence
  (twelve fields each), and every live job — each collected into a fresh `Vec`
  and then handed to a setter.

The sidebar's setters replace unconditionally and apply their bounds *after*
the clone (`src/tui/components/sidebar.rs:381` `set_jobs` does
`jobs.into_iter().take(MAX_SIDEBAR_JOB_ROWS)`, `:442` `set_run_groups` does
`.take(16)`, `:449` `set_agent_tree` does `.take(MAX_SIDEBAR_AGENT_TREE_NODES)`),
so a bounded display does not bound the work.

Each setter then calls `reconcile_focus` (`:758`), which calls `focus_targets`
(`:650`) — a fresh `Vec<SidebarFocusTarget>` with cloned string keys. Four to
five setters per population means four to five such rebuilds per panel per
frame.

### 3.3 The sidebar's own line model is built four times per frame

`src/tui/components/sidebar.rs:2039` `render_with_title`:

```rust
let lines = self.content_lines(area.width);                       // 1
let scroll_offset = self.scroll_offset.min(self.max_scroll(area)); // 2 (max_scroll -> content_lines)
...
if self.max_scroll(area) > 0 {                                     // 3
    let mut state =
        ScrollbarState::new(self.content_lines(area.width).len())  // 4
```

`max_scroll` (`:970`) is `self.content_lines(area.width).len().saturating_sub(viewport_height)`,
so calls 2 and 3 each rebuild the entire model. Each `content_lines` call
materializes every section — session, git, config, goal, plan items, todos,
subagents, jobs, MCP servers, file changes, tool programs — allocating a `Line`
and its span strings per row.

### 3.4 An open subagent tab is deep-cloned per frame

`src/tui/app/render.rs:1581`:

```rust
let Some(tab) = self.ui_state.subagents.active_tab().cloned() else { return };
```

`SubagentTabState` (`src/tui/app/state/subagent_tabs.rs:169`) carries
`row: SubagentRow` and `transcript: Option<TranscriptCache>`. The cache is
bounded at `MAX_TRANSCRIPT_CACHE_ENTRIES = 512`
(`src/tui/app/state/transcript_cache.rs:42`), and `SubagentRow` itself carries
`transcript: Option<AgentRunTranscriptProjection>`, which can hold a second
entry list. So one frame with a subagent tab open can deep-copy up to two
512-entry transcript windows, then `delegated_transcript_lines` (`:1775`)
rebuilds lines for all of them to paint roughly forty rows.

### 3.5 The composer hint line is built twice per frame

`src/tui/app/render.rs:207`:

```rust
let hint_rows = u16::from(self.composer_hint_text().is_some());
```

`composer_hint_text` (`src/tui/app/mod.rs:13160`) takes `&mut self` and builds a
`String`, and its bounded form dispatches through resize mode, the chat seed,
the editor's route, the LLM buffer's key hints, and the active panel's
`composer_hints`. The frame then calls `composer_hint_text_bounded(budget)`
again at `:2190` to actually draw the row. The first call exists only to answer
"is it `Some`".

### 3.6 The status bar is rebuilt from scratch per frame

`src/tui/app/render.rs:2564`–`:2593` builds a `TuiStatusSummary` (`:2596`,
allocating several `String`s and a `Vec` of activity chips), formats a token
line, then calls `set_theme`, `apply_summary`, `set_project_context`,
`set_policy_status` and `set_lsp_status(self.lsp_status_cache.clone())`.

`apply_summary` (`src/tui/components/status_bar.rs:133`) clones `primary`,
`undo_message`, `secondary` and every chip into the widget's own fields;
`set_project_context` (`:83`) runs `middle_truncate` over a fresh `String`
every frame.

### 3.7 The tasks overlay's rows are built twice per frame, with an allocating lookup

`src/tui/app/mod.rs:15313` `tasks_strip_height` returns
`self.bounded_task_rows().len()`, and `bounded_task_rows` (`:14994`) is
`tasks::bounded(self.task_rows())`. `App::render` calls `tasks_strip_height()`
at `:240` to size the strip; `render_tasks_strip` (`:1394`) then calls
`self.task_rows()` again at `:1404`.

`task_rows` (`:15323`) additionally runs a linear
`session_work.iter().find(..)` per subagent row, and the comparison builds
`format!("task:{task_id}")` **inside the predicate** (`:15367`) — one `String`
allocation per candidate compared. `select_session_work`
(`src/tui/app/state/session_work.rs:45`) is called by `task_rows` and again by
`sync_subagents_from_snapshot` (`src/tui/app/render.rs:3070`), building three
`HashSet`s and several `format!` keys per node on each call.

`sync_subagent_rows_from_projection` (`:3052`) runs every frame and funnels
into `apply_subagent_rows` (`src/tui/app/mod.rs:5719`), which calls
`on_snapshot_watermarks` (`src/tui/commands/transcript_follow.rs:702`) — a
per-frame allocation of a watermark `Vec` (cloning every `run_id`) and a tab-id
`Vec`.

### 3.8 The measured consequence

`src/tui/runtime/event_loop.rs` sets `needs_render = true` on every pump tick
while streaming, while the session is `Working`, or while a toast is showing,
and the streaming interval is 16 ms with an 80 ms spinner interval. Every cost
above is therefore paid 12.5–62 times per second.

## 4. Invariants that must not regress

- **The frame cadence, the spinner table, `ActivityPhase`, and the render's
  `catch_unwind` are unchanged.** No file in `src/tui/runtime/` is touched.
  `scripts/check_tui_render_cost.py` pins the `catch_unwind` and the 16 ms
  interval and will fail if either moves.
- **The rendered output is byte-identical** for a given state. This milestone
  changes *when* a value is computed and *how often*, never *what*.
- **A drag selection still copies exactly what was on screen.** The retained
  buffer must still exist for every frame drawn while a selection is live.
- **The sidebar still shows the current projection on the frame it arrives.**
  Deferring the population must not delay a visible update by a frame.
- **The tasks overlay still shows the same rows, in the same order, with the
  same "N more" counts.** The proportionality of the full list's construction
  is accepted (see §5).
- **One producer per derived value.** The transcript's row count still comes
  from the layout cache; the composer's rows still come from one wrap.

## 5. Scope

### In scope

- `App::finish_render`'s retained-buffer clone.
- `App::populate_session_panel` and its call site in `render_panel_at`.
- `SidebarWidget::render_with_title`'s repeated `content_lines` /
  `max_scroll` calls.
- `App::render_subagent_view`'s `active_tab().cloned()`.
- The composer hint line's double build.
- `App::render_footer` and the `StatusBarWidget` setters it drives.
- `App::tasks_strip_height`, `App::bounded_task_rows`, `App::task_rows`, and
  `render_tasks_strip`'s duplicate build.
- `sync_subagent_rows_from_projection` / `apply_subagent_rows` /
  `on_snapshot_watermarks` running every frame.
- A counter for each shape above, in `src/tui/render_cost.rs`, with an
  assertion in `tests/tui_render/tui_render_cost.inc`.

### Explicitly out of scope

- `src/tui/runtime/**` — the event loop, the cadence, the spinner.
- The transcript's retention policy and the layout cache's second copy
  (C006's recorded decision; see `architecture/tui.md` §"The transcript's
  retention").
- The tasks overlay's **proportionality** — its full-list construction is
  required because each group header's "N more" needs the group's total. This
  milestone makes the build happen **once** instead of twice, and stops the
  per-comparison allocation. It does not make the build sublinear.
- The sidebar's own row bounds (`MAX_SIDEBAR_*`). They stay as they are.
- The chat, file-tree, change-review, editor and wrap surfaces — M002–M006.
- Any user-visible change.

## 6. Required production changes

### Core/domain

- `finish_render` must retain the buffer **only while a selection is live**.
  The `else` arm is deleted. The selection arm keeps its clone and its
  highlight pass.
- `populate_session_panel` must run **at most once per frame**, and only when
  the sidebar needs it. The implementer must choose between (a) hoisting the
  call above the per-placement loop, and (b) gating it on a projection
  revision plus the sidebar's own state. Option (a) is the minimum and is
  acceptable; option (b) is preferred, because a frame that neither changed
  the projection nor the session should do no work at all. Whichever is
  chosen, the sidebar must still reflect a projection that arrives on this
  frame.
- The sidebar's setters must not be handed a fully-cloned collection that is
  then truncated. Either the bound moves before the clone, or the setter takes
  a borrowing form. The visible result is identical either way, because the
  truncated tail is not displayed — **except** where a setter's total count
  depends on the full length (`set_jobs` records `job_total`, `set_subagents`
  records `subagent_total`). Those totals must be computed without cloning the
  rows.
- `reconcile_focus` must not be run four to five times per frame. Either the
  setters stop calling it individually and the population step calls it once at
  the end, or `focus_targets` stops cloning keys. The first is preferred: it
  also removes the repeated `Vec` allocation.
- `SidebarWidget::render_with_title` must build `lines` once and derive the
  scroll clamp and the scrollbar's content length from `lines.len()`.
- `render_subagent_view` must borrow the active tab rather than clone it. The
  implementer must handle the borrow conflict with the `&mut self` calls in the
  view; the accepted shape is to read the fields it needs (title, status,
  liveness, description, profile, usefulness, scroll, follow state, and a
  borrowed entry iterator) into locals before any mutable use, or to split the
  function into a borrow-only line builder and a mutable writer.
- The composer hint line must be built once per frame. The implementer must
  decide which call wins: the height computation needs only `is_some()`, so the
  cheapest correct shape is a side-effect-free predicate (e.g.
  `has_composer_hint()`) used for `hint_rows`, and the bounded build used for
  the draw. A predicate that duplicates the dispatch is acceptable only if it
  cannot disagree with the builder; the preferred shape is one function that
  returns the bounded `Option<String>` and a height derived from its
  `is_some()`, with the string carried to the draw site.
- `render_footer` must not rebuild the summary when nothing it depends on
  changed, and must not clone the LSP cache per frame. The implementer must
  choose between (a) caching the summary against the fields it reads, and (b)
  having the status bar borrow instead of own. Option (a) is smaller and is
  preferred; if the field set is too broad to key safely, (b) is the fallback
  and the plan's §14 stop condition applies.
- `tasks_strip_height` must not build the row list. The height is a function of
  the bounded row count; the implementer must either cache the built rows on
  `App` for the frame (one build, two readers) or compute the count from the
  same `bounded` allowance without materializing `TaskRow`s. The first is
  preferred because it also serves `render_tasks_strip`.
- `task_rows`' per-subagent lookup must stop allocating per comparison. The
  `work.key` comparison must be done against a borrowed or precomputed key, and
  the lookup must be indexed rather than linear where the row count makes it
  worth it.
- `on_snapshot_watermarks` must not allocate its watermark and tab-id vectors
  on a frame where neither the snapshot nor the tab set changed. Gate it on the
  same revision that gates the population step.

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

None. Every cache here is per-widget and touched only from the render thread.
If a change would need a lock or a shared cell across threads, stop — that is
§14.

### Frontend or operator surface

- The rendered output must be byte-identical. The comparison is §10.

### Security and authorization

No boundary changes. One check: the sidebar holds **file paths** (via
`changed_files` and the git state). This milestone does not add a cache of
paths — it removes clones — so no new path lifetime is introduced. The
implementer must confirm that the only new retained state is derived from data
the sidebar already held for the frame.

### Documentation and static guards

- `architecture/tui.md` §"The render path's caches (C006)": add a row per cache
  introduced here (the sidebar projection gate, the per-frame row build, the
  summary cache), with its key and what drops it.
- `src/tui/render_cost.rs`: new counters, each with a producer on the render
  path.
- `tests/tui_render/tui_render_cost.inc`: an assertion per counter.
- `scripts/check_tui_render_cost.py`: extend only for shapes that would fail
  **silently** — in particular, a restored unconditional `last_frame_buffer`
  clone and a restored `populate_session_panel` call inside the per-placement
  loop. Add each to `--self-test`.

## 7. Ordered work packages

### WP1 — Capture the baseline counts

Intent: every later claim needs a before.

Required changes: add the counters first, with no behaviour change, and record
the baseline numbers by running the new assertions against the unmodified code
(the assertions are expected to fail; the failure output is the baseline). The
counters are:

- `frame_buffer_clones` — one per `last_frame_buffer` assignment;
- `sidebar_populations` — one per `populate_session_panel` body entry;
- `sidebar_line_builds` — one per `content_lines` call;
- `subagent_tab_clones` — one per `active_tab().cloned()`;
- `composer_hint_builds` — one per hint build;
- `status_summary_builds` — one per `build_status_summary`;
- `task_row_builds` — already exists (`render_cost::count_task_row_build`);
  verify it counts both builds;
- `watermark_scans` — one per `on_snapshot_watermarks` body entry.

Acceptance evidence: the baseline counts are recorded per state — idle, a
streaming frame, a frame with one panel open, a frame with two panels open, a
frame with a subagent tab open, a frame with the tasks overlay open.

### WP2 — Stop retaining the buffer when nothing can read it

Intent: the single largest unconditional per-frame copy.

Required changes: delete the `else` arm of `finish_render`'s retention.

Acceptance evidence: `frame_buffer_clones` is 0 on a frame with no selection
and 1 on a frame with a selection; a drag-and-copy still produces the same
clipboard text as before (an existing or new test over `copy_selection_to_clipboard`
with a fixture buffer).

### WP3 — Populate the sidebar once, and only when it moved

Intent: stop re-materializing the projection per panel per frame.

Required changes: hoist and gate the population call; move the bounds before
the clones; collapse `reconcile_focus` to one call per population; build the
sidebar's line model once.

Acceptance evidence: `sidebar_populations` is at most 1 per frame with one or
two panels open and 0 on a frame where the projection and session did not
change; `sidebar_line_builds` is 1 per frame that draws the sidebar; the
sidebar's rendered buffer is identical for the fixture set.

### WP4 — Borrow the subagent tab

Intent: remove up to two 512-entry transcript copies per frame.

Required changes: as §6 describes.

Acceptance evidence: `subagent_tab_clones` is 0; the subagent view's rendered
buffer is identical for a fixture tab with a populated transcript cache.

### WP5 — Build the hint line once

Intent: remove a duplicate string build whose only purpose was `is_some()`.

Required changes: as §6 describes.

Acceptance evidence: `composer_hint_builds` is at most 1 per frame; the hint
row's rendered buffer is identical across the routes and modes the dispatch
covers (resize mode, chat seed, editor route, LLM buffer during a turn, an
active panel).

### WP6 — Stop rebuilding the status bar summary

Intent: remove per-frame string and chip allocation on the footer.

Required changes: as §6 describes.

Acceptance evidence: `status_summary_builds` is 0 on a frame where no field
the summary reads changed, 1 when one does; the footer's rendered buffer is
identical.

### WP7 — Build the tasks overlay's rows once

Intent: remove the duplicate build and the per-comparison allocation.

Required changes: as §6 describes.

Acceptance evidence: `task_row_builds` counts one build per frame with the
overlay open (down from two); the overlay's rendered buffer is identical,
including each header's "N more" count.

### WP8 — Gate the watermark scan

Intent: remove a per-frame allocation on a path with no consumer change.

Required changes: gate `on_snapshot_watermarks` on the same revision as the
population step.

Acceptance evidence: `watermark_scans` is 0 on an unchanged frame and 1 when
the snapshot moves; the live-follow behaviour is unchanged (the transcript
follow still starts for a dirty tab — an existing test in
`tests/tui_render` covers the follow, or a new one is added).

### WP9 — Prove the rendering is unchanged, and prove the counters bite

Intent: the milestone's primary acceptance property, plus the guarantee that
the new assertions are not vacuous.

Required changes: a render comparison over a fixture set — idle frame, a frame
with a sidebar, a frame with two panels, a frame with a subagent tab, a frame
with the tasks overlay, a frame with a live selection — asserting byte-identical
output between the warm and cold paths. Then perturb one cache key deliberately
(e.g. make the sidebar gate ignore the projection revision) and confirm the
comparison fails.

Acceptance evidence: the comparison passes on the shipped tree; the perturbed
comparison fails with a legible diff; the perturbation is reverted and recorded
in the closure record the way C006 recorded its own.

## 8. Failure, cancellation, restart, and contention semantics

- **Failure.** A gate that is too coarse shows a stale sidebar; a gate that is
  too fine does no work but also no update. The gate must be a function of
  exactly the values the population reads. A projection that arrives mid-frame
  must be reflected on that frame or the next, never dropped.
- **Cancellation.** A session teardown clears the sidebar and the retained
  buffer. No state introduced here survives a session.
- **Restart.** Nothing durable. All state is in-memory and starts empty.
- **Contention.** None: every value is owned by one widget on one thread.
- **A selection that is cleared.** `clear_selection` must also drop
  `last_frame_buffer`, so a stale frame is not retained after the interaction
  it existed for ends.

## 9. Compatibility and migration

None. No protocol, storage, configuration, or migration change. No public API
changes outside `src/tui/`. `last_frame_buffer`'s field type does not change;
only its assignment discipline does.

## 10. Required tests

### Focused unit tests

- A frame with no selection leaves `last_frame_buffer` unset; a frame with a
  selection sets it.
- `copy_selection_to_clipboard` over a fixture buffer extracts the same text
  before and after the change.
- The sidebar population is invoked at most once per frame with two panels
  open.
- The sidebar population is not invoked when the projection revision and the
  session are unchanged.
- `content_lines` is invoked once per `render_with_title`.
- `active_tab` is not cloned in `render_subagent_view`.
- The composer hint is built at most once per frame, across the dispatch's
  branches.
- The status summary is not rebuilt on an unchanged frame.
- `task_rows` is built once per frame with the overlay open.
- `task_rows`' subagent lookup allocates no `String` per comparison.

### Integration tests

- A streaming frame over the render path performs the expected counts for every
  new counter, driven through `tests/tui_render` as C006's cost member does.

### Restart and recovery tests

None required. Nothing durable changes.

### Contention and cancellation tests

- Clearing a selection drops the retained buffer.

### Security and negative tests

- A sidebar populated from a projection that has been replaced shows the new
  rows, not the old ones (the gate must not serve a stale projection).

### Migration and compatibility tests

None.

### Placement and vacuity

`tests/tui_render/tui_render_cost.inc` is the home for the counts; a new member
(`tests/tui_render/<name>.inc` bound in `tests/tui_render/mod.rs` with
`#[path = "<name>.inc"] pub mod <name>;`) is acceptable if the file grows past
its present scope. Every new assertion must be shown failing against the
baseline — a counter that cannot fail is not evidence.

## 11. Required verification commands

```bash
# The new counters and the render comparison.
cargo test --test tui_render -- tui_render_cost

# The whole family.
cargo test --test tui_render

# The touched modules' own unit tests.
cargo test -p codegg --lib -- tui::app::render tui::components::sidebar \
    tui::components::status_bar tui::app::state::tasks \
    tui::commands::transcript_follow

# The render-cost shape guard, and the proof it bites.
python3 scripts/check_tui_render_cost.py
python3 scripts/check_tui_render_cost.py --self-test

# Rust-aware test inventory (ground truth; never regex-scan Rust).
python3 scripts/rust_test_scan.py --validate

# Scoped: quick's guards plus only the tests this change can affect.
scripts/verify.sh scoped HEAD~1

# Before the closure record is written.
bash scripts/verify.sh full
```

`scripts/verify.sh scoped` widens to every transitive dependent's surface for a
root-package change; C006 recorded that it did not finish inside one milestone's
budget. If it does not finish, say so and run the affected family and unit
tests directly rather than claiming it.

## 12. Documentation updates

- `architecture/tui.md` §"The render path's caches (C006)": new table rows for
  the sidebar projection gate, the per-frame row build, and the summary cache.
- `architecture/tui.md` §"Invariants & Gotchas", only if the change makes a
  previously implicit rule explicit (e.g. "the frame buffer is retained only
  while a selection is live"). Do not restate the cadence rules.
- No `docs/` change: nothing user-visible changes.

## 13. Acceptance criteria

Externally observable or contract-level:

1. A frame with no text selection performs **zero** terminal-buffer clones.
2. A frame with a text selection retains the buffer, and a drag-copy produces
   the same clipboard text as before the change.
3. With one or two panels open, the sidebar's projection is populated **at most
   once** per frame, and **zero** times on a frame where neither the projection
   nor the session changed.
4. The sidebar's line model is built **once** per frame that draws it.
5. `render_subagent_view` performs **zero** `SubagentTabState` clones per frame.
6. The composer hint line is built **at most once** per frame.
7. The status summary is rebuilt **zero** times on a frame where nothing it
   reads changed.
8. The tasks overlay's rows are built **once** per frame, down from twice, and
   its subagent lookup allocates no `String` per comparison.
9. The rendered output is byte-identical to the baseline for every state in the
   fixture set, and the comparison is demonstrated capable of failing.
10. The frame cadence, the spinner, and the render's `catch_unwind` are
    unchanged, verified against the diff and by the shape guard.

## 14. Stop conditions

Stop and report rather than improvise if:

- A change would alter the rendered output. This is an optimisation; a rendering
  change is a different milestone.
- A gate cannot be given a correct invalidation, or the invalidation cannot be
  proven by the render comparison. A stale sidebar is a correctness defect.
- The status summary's key cannot be expressed without either reading a field
  that changes every frame (making the cache useless) or widening into a second
  ownership boundary. Report and leave the footer as it is.
- Borrowing the subagent tab would require restructuring `render_subagent_view`
  beyond a split into a line builder and a writer — for example if the view
  would need to hold a borrow across a `&mut self` call in a way that cannot be
  expressed. Report the shape rather than cloning the tab a different way.
- A change would need a lock, an `Rc<RefCell<_>>` shared across threads, or any
  state that outlives the frame's widget.
- A change would touch `src/tui/runtime/**`, the spinner, `ActivityPhase`, or
  the `catch_unwind` boundary.
- The tasks overlay's proportionality would have to change to meet criterion 8.
  It must not: the "N more" count needs the group's total, and C006 recorded
  that reason.

## 15. Closure evidence required

- The baseline counts and the after-counts, per counter, with the measurement
  method stated (operation counts, not wall-clock).
- The render comparison's result, and the demonstration that it can detect a
  difference.
- The confirmation that `src/tui/runtime/**` is untouched, verified against the
  diff.
- The shape guard's output and its `--self-test` output.
- `cargo test --test tui_render`, the touched modules' unit tests, and
  `scripts/verify.sh quick`; `scoped` and `full` labelled local versus CI, and
  `scoped` named as unfinished if it is.
- The registry update, and the unblock audit of the registry's Blocked work
  section (this milestone is expected to unblock nothing).

## 16. Handoff notes

- **The buffer clone is the whole milestone's headline.** It is four lines to
  remove and it is the largest single per-frame cost in the TUI. Do it first,
  measure it, and keep the measurement.
- **`populate_session_panel` runs inside the per-panel loop**, not once per
  frame. That is the defect. Read the loop at `src/tui/app/render.rs:402` before
  reading the population function.
- **The bounds are applied after the clone.** Moving a bound earlier is safe
  only where the discarded tail is unused. `set_jobs` and `set_subagents` keep
  a *total*, and that total must still be computed — without cloning the rows.
- **`reconcile_focus` is called by every setter.** Collapsing it to one call
  per population is the fix; removing it from the setters individually would
  break the setters' other callers.
- **The subagent tab clone is two transcripts, not one.** `row.transcript` and
  `transcript` are separate fields and both can hold entry lists.
- **The hint line's `is_some()` is the only reason the first build exists.**
  Do not "optimise" the builder; remove the duplicate call.
- **Do not make the tasks overlay sublinear.** Its full-list construction is a
  recorded, justified cost.
- **`scripts/check_tui_render_cost.py` is the right home for the shapes that
  fail silently** — an unconditional buffer clone, a population call inside the
  loop. A test would catch them too, but the guard is what runs when nobody
  remembers.
