# TUI Render Cost Milestone 001 — Closure Status

Status: **closed** (2026-11-10), with one recorded deferral (§8).

Implementation plan:
`plans/implementation/tui-render-cost/001-frame-composition-cost.md`
Source roadmap: `plans/subsystems/tui-render-cost-roadmap.md` §7 (M001)
Repository baseline: `01c45971d4330cfd1bf6541510de1eeef90b7dbc`
No ADR was required, and none was created.

## 1. Executive finding

A drawn frame paid for work no interaction could spend. The largest item was a
deep clone of the whole terminal buffer on **every** frame — `width × height`
cells, each a `Cell` with a symbol, two colours, an underline colour and a
modifier set — made for a reader that returns early unless a text selection
exists. Alongside it the sidebar's projection was re-materialized once per
occupied panel per frame, the sidebar rebuilt its whole line model four times,
an open subagent tab was deep-cloned (up to two 512-entry transcript windows),
the composer hint line was built twice, and the tasks overlay's rows were built
twice with an allocating lookup inside the comparison.

The frame cadence is the contract, not the defect: `src/tui/runtime/event_loop.rs`
forces a redraw on every pump tick while streaming, while a turn is `Working`,
or while a toast shows, so with a 16 ms streaming interval and an 80 ms spinner
interval every cost above was paid 12.5–62 times per second. The fix was
therefore to take the work off the frame, never to slow the frame down.

## 2. The cost sites, before and after

Operation counts, per frame, measured through the counters in
`src/tui/render_cost.rs`. No wall-clock claim is made anywhere.

| Site | Counter | Before | After |
|---|---|---|---|
| `finish_render`'s retained buffer, no selection | `frame_buffer_clones` | 1 | **0** |
| `finish_render`'s retained buffer, selection live | `frame_buffer_clones` | 1 | **1** (kept) |
| Sidebar population, two panels open | `sidebar_populations` | 2 | **1** |
| Sidebar population, unchanged frame | `sidebar_populations` | 1 | **0** |
| Sidebar population, projection replaced | `sidebar_populations` | 1 | **1** (must not be dropped) |
| `SidebarWidget::render_with_title` | `sidebar_line_builds` | 3 | **1** |
| `render_subagent_view`, tab open | `subagent_tab_clones` | 1 | **0** |
| Composer hint line | `composer_hint_builds` | 2 | **1** |
| Status-bar summary | `status_summary_builds` | 1 | 1 (**deferred**, §8) |
| Tasks overlay rows | `task_list_builds` | 2 | **1** |
| Transcript watermark scan, unchanged snapshot | `watermark_scans` | 2 | **0** |
| Transcript watermark scan, snapshot moves | `watermark_scans` | 2 | **1** |

### 2.1 The captured baseline failures

Captured by adding the counters with no behaviour change and running the new
assertions against the unmodified render path (WP1). Seven of sixteen failed;
the failure output is the before-count.

```
left: 1  right: 0   an_unchanged_frame_does_not_repopulate_the_sidebar
left: 2  right: 1   the_tasks_overlay_builds_its_rows_once_per_frame
left: 2  right: 1   the_composer_hint_is_built_once_per_frame
left: 3  right: 1   the_sidebar_builds_its_line_model_once_per_frame
left: 2  right: 0   an_unchanged_frame_does_not_rescan_the_watermarks
left: 1  right: 0   an_open_subagent_tab_is_not_cloned_per_frame
left: 2  right: 1   two_open_panels_populate_the_sidebar_once_per_frame
"a frame nobody can drag over must not keep a copy of itself"
"the buffer existed for the selection, so it goes with the selection"
```

## 3. The equivalence argument, per change

- **The retained buffer.** The `else` arm of `finish_render` is deleted, so
  `last_frame_buffer` is assigned only inside the selection arm. The selection
  arm is byte-identical to before: it applies the highlight pass over the same
  area and then clones. `clear_selection` now also drops the buffer, so the
  retained copy cannot outlive the interaction it exists for. The buffer is
  therefore present for every frame drawn while a selection is live and absent
  otherwise — which is exactly the set of frames `copy_selection_to_clipboard`
  can read.
- **The sidebar gate.** `populate_session_panel_if_needed` returns early only
  when **both** the projection's `(stream_id, event_seq)` **and** every
  session-derived input the population copies already match what the sidebar
  holds. The comparison is against the widget's own stored rows, so it allocates
  nothing. A projection that arrives is always reflected on that frame, because
  its revision is the first half of the test. The population is hoisted above the
  per-placement loop and gated on the Session panel actually being on screen, and
  it keeps its own `catch_unwind` — the containment the per-panel wrapper used to
  provide.
- **The sidebar's line model.** `render_with_title` builds `lines` once and
  derives both the scroll clamp and the scrollbar's extent from `lines.len()`,
  reproducing `max_scroll`'s own arithmetic (`sidebar_content_height` +
  `content_lines(area.width).len()`).
- **The subagent tab.** `render_subagent_view` borrows the active tab. The borrow
  ends before the scroll write-back; the accepted shape was to read the fields the
  view needs before any mutable use.
- **The hint line.** One build, at the point the row's height is decided, carried
  to the draw site. The budget at that point is `pane_area.width - 4`, and the
  hint chunk is a vertical split of the pane, so it is the same budget the draw
  used. `is_some()` is budget-independent (the builder returns `None` only when
  the part list is empty, which `fit_parts` cannot change), so the reserved row
  count is unchanged.
- **The tasks overlay.** `build_frame_task_rows` derives the full list and the
  bounded rows once; the height reads the bounded count and the painter reads
  both. The full list is still built because each group header's "N more" needs
  its group's total — the proportionality is a recorded, justified cost, not a
  shortfall. The subagent lookup compares `work.key.strip_prefix("task:")` against
  `task_id` instead of building `format!("task:{task_id}")` per candidate.
- **The watermark scan.** Gated on `(stream_id, event_seq, tab count, active tab)`
  **before** the watermark vector is built, so an unchanged frame allocates
  neither vector.

## 4. Invariant review

- **The frame cadence, the spinner, `ActivityPhase` and the render's
  `catch_unwind` are unchanged.** `git diff --stat -- src/tui/runtime` is empty,
  and `scripts/check_tui_render_cost.py` pins both the `catch_unwind` and the
  16 ms interval.
- **The rendered output is byte-identical.** The milestone's tests compare warm
  and cold buffers over the fixture set, and the comparison was demonstrated
  capable of failing (§7).
- **One producer per derived value.** The transcript's row count still comes from
  the layout cache; the composer's rows still come from one wrap.
- **The tasks overlay shows the same rows, order and "N more" counts.** The
  overlay's own test pins the true group total.

## 5. Security review

No authorization boundary changes, no new path lifetime, no cache of file paths.
The one new retained state is the sidebar's projection revision — a
`(stream_id, event_seq)` pair — and the widget's own rows, which it already held.
Every cache is per-widget and touched only from the render thread; nothing
survives a session teardown. `clear_selection` drops the retained frame.

## 6. Migration and compatibility review

None. No protocol, storage, configuration or migration change. `last_frame_buffer`'s
type is unchanged; only its assignment discipline moved. `App` gained three
private fields with `None` initializers at both construction sites.
`SidebarWidget` gained `begin_population`/`end_population`/`rows_changed`, two
`set_*_with_total` forms, and `population_is_current`; the existing setters keep
their signatures and their behaviour outside a population.

## 7. Verification executed

```text
$ cargo fmt --check
fmt=0
$ cargo test --test tui_render -- tui_render_cost
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 1118 filtered out
$ cargo test --test tui_render
test result: ok. 1126 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
$ cargo test -p codegg --lib -- tui::app::render tui::components::sidebar \
    tui::components::status_bar tui::app::state::tasks tui::commands::transcript_follow
test result: ok. 67 passed; 0 failed; 0 ignored; 0 measured; 7760 filtered out
$ python3 scripts/check_tui_render_cost.py
check_tui_render_cost: the render path derives each result once
$ python3 scripts/check_tui_render_cost.py --self-test
self-test: 10 injections, all rejected
$ python3 scripts/rust_test_scan.py --validate
rust_test_scan --validate: all checks pass
```

The family count is the count **at this milestone's closure**; it rises with each
later milestone of the roadmap. The tree-level `scripts/verify.sh quick` result is
recorded once, in the roadmap's closure, because it is a property of the whole
roadmap rather than of this milestone.

**The perturbation that proves the gate bites.** Changing the population gate to
ignore the projection revision (`if self.sidebar_inputs_are_current()`) made the
invalidation half of the equivalence test fail with
`left: 0 / right: 1 — "a changed projection revision forces exactly one
population"`. Reverted, re-run green, and recorded here rather than left implicit.

## 8. Unresolved findings and deferrals

- **WP6 — the status-bar summary — is deferred under the plan's §14 third stop
  condition.** A correct key would have to name every field the summary reads
  across `App` (status, error, dialog and permission flags, registry counts,
  projection-derived counts, the changed-file pending count, the agent and
  reasoning strings, the control lease, the formatted goal line) **and** the
  derived `Instant::now() < undo_until` predicate. Computing the key reproduces
  the same collection scans and string formatting as the summary, so the cache
  would remove none of the cost it exists to remove; omitting the time predicate
  would serve a stale undo prompt. Option (b) — the status bar borrowing instead
  of owning — would redesign widget ownership and is the plan's own named
  fallback. `status_summary_builds` and its test pin the remaining 1-per-frame
  cost so a later milestone can take it.
- **`copy_selection_to_clipboard` is not driven end to end in a test**, because
  the OS clipboard is environment-dependent. The test proves the retained buffer
  is byte-identical to the drawn buffer and that `selection::extract` — the
  function the copy path feeds — returns identical text from both. That is the
  extraction half of the drag-copy claim, not the clipboard write.
- **The `task-lookup-key` claim has no allocation counter.** It is enforced
  mechanically by the guard's `task-lookup-key` check (which rejects the literal
  `format!("task:{task_id}")`) plus the full family. An allocation count would
  need a process-global allocator, which is measurement theatre in a test binary
  that runs cases in parallel.
- **Composer-branch coverage** is Insert, `Working` and — since the corrective
  pass of 2026-11-10 — **resize mode**, which outranks every surface hint and so
  builds its text from a different branch. An armed chat seed, the editor route
  and an active panel are still not separately constructed, and the reason is
  recorded: the build happens at **one** call site before the dispatch, so the
  branch cannot change how many builds a frame performs.
- **The retargeted `tasks-whole-list-clone` injection is a real deep clone**
  again (corrective pass, 2026-11-10). WP7 moved its anchor, and the first
  retarget injected `all_rows.clone()` on a **slice**, so it carried the token
  the check greps for and demonstrated nothing. It now binds `built.all.clone()`
  — a `Vec<TaskRow>` — before taking the slice, so the injection copies rows.
  The two checks this milestone added (`frame-buffer-retention`,
  `sidebar-population-per-panel`) were already structural and both bite.
- **`SubagentTabState`'s `Clone` is instrumented** to count copies. It is the
  producer that keeps `subagent_tab_clones` a real hook after the frame's clone
  was removed, at the cost of a counter bump in a `Clone` impl.

## 9. Roadmap disposition

M001 is **closed**. It discharges nothing from
`plans/closure/repository-audit-corrective/006-status.md` §12 — its one carried
finding in this area (the tasks overlay's proportionality) was owned as a
**narrower** claim and is recorded above as still proportional by design. It
unblocks nothing in the registry.

## 10. Registry update

The roadmap's milestone table records M001 as `closed` with this record linked;
`plans/registry.md` moves its row from "Dependency-ready implementation plans" to
the closed disposition with this path. The unblock audit found nothing to
unblock.
