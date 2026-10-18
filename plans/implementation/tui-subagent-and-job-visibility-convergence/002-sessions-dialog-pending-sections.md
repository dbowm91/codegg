# TUI Subagent and Job Visibility Convergence — Milestone 002 — `/sessions` Gains Subagents and Jobs Sections

Status: ready for handoff

Repository baseline: `2acce2138` (plus the uncommitted C001/C003/C004/C006 tree)

Source roadmap: `plans/subsystems/tui-subagent-and-job-visibility-convergence-corrective-addendum.md#c002--sessions-gains-subagents-and-jobs-sections`
Parent roadmap: `plans/subsystems/tui-subagent-and-job-visibility-roadmap.md`
Long-term requirements: `plans/000-long-term-specification.md`
Applicable ADRs: None required.

Primary class: capability, delivering invariant **I1** and capability **K2**.

## 1. Objective

`/sessions` — the surface a user actually opens to find their sessions — lists
the active subagents and the live jobs of the current session, below the session
list, navigable with the same keys, with `Enter` on a subagent opening the same
tab every other surface opens.

## 2. Why this milestone is ready

C001 is closed, so the chord set and the overlay are stable; this milestone adds
no binding of its own. C004's chain tests mean the rows this milestone reads are
now provably name-carrying, so a missing name here would be visible.

## 3. Current implementation evidence

| Fact | Evidence |
|---|---|
| `/sessions` opens a flat modal with no subagent rows | `src/tui/app/mod.rs:6198` → `Dialog::Session`; `src/tui/components/dialogs/session.rs:79-97` |
| The selection is a bare `usize` into the session list | `src/tui/components/dialogs/session.rs:83`, `selected_session()` at `:205` |
| `j`/`k` move that index only | `select_up`/`select_down`, `:293-313` |
| `Enter` resolves a **session** and nothing else | `src/tui/app/mod.rs:4654-4664` |
| The rows to render already exist and are derived once | `App::task_rows()` → `state::tasks::TaskRow`; `sync_subagents_from_snapshot` (`src/tui/app/render.rs:2569`) |
| "Live" has exactly one predicate | `state::tasks::is_live_state` — the overlay and the sidebar both use it |
| The dialog is rendered from the **focus-manager clone**, not the App-owned field | `commands/sessions.rs:2670-2677` (the exact bug that made `/sessions` paint nothing) |
| The subagent tab opens through one message | `TuiMsg::OpenSubagentTab { task_id }` |

## 4. Invariants that must not regress

- **I1** — every row rendered originates in the projection snapshot. No name is
  minted, no status is inferred.
- **I2** — `scripts/check_tui_transport_authority.py` stays green.
- **I5** — opening a subagent row opens a view; it cancels nothing.
- **Selection is id-keyed.** Focus must not follow list position; a row that
  disappears must not move the cursor onto a different entity.

## 5. Scope

**In scope**

1. Sectioned `Subagents` and `Jobs` blocks in `SessionDialog`, fed from the same
   derived row sources the overlay and sidebar read.
2. A row-type-aware cursor that walks sessions → subagents → jobs, skipping
   headers.
3. `Enter` on a subagent row opening the tab; `Enter` on a job row consumed.
4. A snapshot-refresh path so the sections are not frozen at open time.

**Out of scope**

- Persisting, resizing, or reordering these rows.
- Any job transcript or detail view (there is none; the row is read-only).
- `/sessions` bulk-mode semantics over non-session rows.

## 6. Required production changes

**`src/tui/components/dialogs/session.rs`**

- Add owned row types (`SubagentListRow`, `JobListRow`) plus a
  `SessionDialogRow` enum with a `Header` variant that is **not** selectable.
- `rows()` builds one flat, ordered vector: sessions, then the `Subagents`
  header and its rows **only if non-empty**, then `Jobs` likewise.
- `select_up`/`select_down` walk that vector and skip headers, clamping at both
  ends. The current implementation indexes `sorted_sessions()`; it must index
  `rows()` instead, with the session indices still leading so
  `selected_session()` keeps working unchanged.
- Render the two sections under the session list, reusing the existing
  selection styling.
- Keep `scroll` coherent with the new length.

**`src/tui/app/mod.rs`**

- Fill the sections on open, and on each snapshot refresh while the dialog is
  mounted.
- **Both instances must receive the rows** — the App-owned `dialog_state` field
  *and* the focus-manager clone — for the reason recorded at
  `commands/sessions.rs:2670-2677`. This is the single most likely way to ship
  this milestone looking broken.
- `Dialog::Session` in the `Send` arm (`:4654`): dispatch on the selected row's
  type before the existing session branch.

**Jobs** are filtered with `state::tasks::is_live_state` and keyed by `job_id`,
never by list index.

## 7. Ordered work packages

**WP-1 — Row model and navigation**, with the dual-instance fill, before any
rendering change. *Acceptance:* a test shows the mounted dialog reporting the
subagent row.

**WP-2 — Rendering** of the two sections, with empty sections contributing no
header and no row.

**WP-3 — `Enter` dispatch.** Subagent → `TuiMsg::OpenSubagentTab`. Job →
consumed, with no destination invented.

**WP-4 — Live refresh**, so a subagent that starts while `/sessions` is open
appears without reopening.

**WP-5 — Docs**: `docs/tui.md` and the `architecture/tui.md` disambiguation note
about the two "sessions" surfaces.

## 8. Failure, cancellation, restart, contention semantics

A snapshot that arrives replaces the sections; a *missing* snapshot does not,
matching the strip's existing rule. A subagent that finishes mid-scroll drops
its row and the cursor falls back to the nearest remaining selectable row by
**id set membership**, not by clamped index.

## 9. Compatibility and migration

None. No protocol, storage, or configuration change.

## 10. Required tests

- Sections appear with rows and are absent without them.
- `j`/`k` walk sessions → subagents → jobs and never land on a header.
- `Enter` on a subagent row resolves the **same** tab as the sidebar's `Enter`
  for the same `task_id`.
- `Enter` on a job row changes nothing and closes nothing.
- A row disappearing does not move the cursor onto a different entity.
- The mounted focus-manager instance carries the rows (regression for the
  historical empty-dialog bug).
- No regressions: `cargo test --test tui_side_panels`, `--test tui_main_buffer`.

## 11. Required verification commands

```bash
scripts/verify.sh quick
cargo test --test tui_side_panels
cargo test --test tui_main_buffer
python3 scripts/check_tui_transport_authority.py
cargo fmt
```

## 12. Documentation updates

- `docs/tui.md` — the `/sessions` surface lists subagents and jobs.
- `architecture/tui.md` — the two-"sessions" disambiguation, now resolved.
- `plans/registry.md` — register C002 and its closure record.

## 13. Acceptance criteria (externally observable)

1. `/sessions` shows the session's named subagents and live jobs beneath its
   sessions, or shows neither section when there are none.
2. `j`/`k` reach every section and stop on nothing.
3. `Enter` on a subagent opens the same transcript tab the overlay and sidebar
   open.
4. Nothing in this milestone implies that a subagent can be *messaged*.

## 14. Stop conditions

Report rather than improvise if:

- Feeding the dialog requires a **second** derivation of subagent rows. The
  boundary is one derived vector rendered in more places.
- Making the sections live requires the dialog to hold the projection client.
- Any proposal arises to give a job row a destination.
- Row-type-aware selection cannot be added without changing the meaning of
  `selected` for session navigation or bulk mode.

## 15. Closure evidence required

`plans/closure/tui-subagent-and-job-visibility-convergence/002-status.md` with
the requirement-to-evidence matrix, the dual-instance regression proof, and
truthful local-vs-CI labels.

## 16. Handoff notes

The highest-risk detail is not the row model, it is the **two instances**. The
focus manager holds a clone, `render_dialog` draws that clone, and a fill that
touches only `dialog_state.session_dialog` produces a dialog that looks
correct in every test that reads the App-owned field and empty on screen. The
existing test at `commands/sessions.rs:2676` is the pattern to copy.