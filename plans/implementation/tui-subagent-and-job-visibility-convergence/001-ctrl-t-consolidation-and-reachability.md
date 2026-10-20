# TUI Subagent and Job Visibility Convergence — Milestone 001 — `Ctrl+T` Consolidation and Reachability

Status: closed

Closure record: `plans/closure/tui-subagent-and-job-visibility-convergence/001-status.md`

Repository baseline: `2acce2138`

Source roadmap: `plans/subsystems/tui-subagent-and-job-visibility-convergence-corrective-addendum.md#c001--one-chord-ctrlt-consolidates-todos-subagents-and-jobs`
Parent roadmap: `plans/subsystems/tui-subagent-and-job-visibility-roadmap.md`
Long-term requirements: `plans/000-long-term-specification.md`
Applicable ADRs: None required.

Primary class: capability, delivering invariants **I3** and **I6**.

## 1. Objective

One key opens everything the session is waiting on — todos, subagents, and jobs
— showing only the groups that have rows, and it works in the mode the app boots
in.

## 2. Why this milestone is ready

No dependencies. M001–M005 shipped in `c0d6ff2d3`; the combined block's row
model (`build_task_rows`) and its empty-group omission are already correct and
are **reused, not rewritten**.

This milestone deliberately lands before C002 (`/sessions` sections) and C003
(`/agents`), because both are entry points a user reaches first and neither
should be built against a chord set that is about to change.

## 3. Current implementation evidence

| Fact | Evidence |
|---|---|
| `Ctrl+T` → `ToggleTodoList` in all three keymaps | `src/tui/input.rs:768-771`, `:893-896`, `:1059-1062` |
| Two tests assert that mapping | `src/tui/input.rs:2529`, `:2570` |
| `Shift+T` → `ToggleTasksList`, SHIFT and NONE | `src/tui/input.rs:674-681` |
| `ToggleTasksList` is `is_normal_only` **only** because `Shift+T` is a capital | `src/tui/input.rs:580-584` |
| `InputMode` defaults to `Insert` | `src/tui/input.rs:73-78` |
| `Ctrl+T` is not in `is_normal_only` — it works in both modes today | `src/tui/input.rs:561-599`; help lists it in both sections (`:1231-1236`, `:1589-1594`) |
| Combined row model + omission already correct | `src/tui/app/state/tasks.rs:143-194`, `build_task_rows` |
| `tasks_strip_height` is mode-independent and already clamped | `src/tui/app/mod.rs:14596-14601`; `src/tui/app/render.rs:1323-1342` |
| Compact todo strip returns 1 row whenever todos exist | `src/tui/app/mod.rs:14462-14474` |
| Editor route claims `Ctrl+T` for the file tree | `src/tui/commands/editor.rs:1849-1853`, intercepted at `src/tui/app/mod.rs:3988-3994` |
| Stale `Ctrl+T` Normal help row says "Toggle sidebar" | `src/tui/input.rs:1589-1594` |
| Guard only asserts a key resolves to *something* | `src/tui/input.rs:2765-2816` (`:2799-2806`) |
| `help_overlay.rs` undeclared, never compiled | absent from `src/tui/components/mod.rs:56-74` |

## 4. Invariants that must not regress

- **I1** — projection-derived rows; no client-side fabrication.
- **I2** — panel input authority; `scripts/check_tui_transport_authority.py` green.
- **I3** — no bare `Ctrl+<letter>` is introduced. `Ctrl+T` is already a bound
  bare-Ctrl chord, so this milestone **rebinds an existing chord** rather than
  adding one; `cargo test --test modifier_chords` must stay green.
- **I4** — working strip row 0 reserved.
- **I5** — a tab cancels nothing.
- **I6** *(new, established here)* — every advertised chord resolves to the
  action it advertises, in the mode it is advertised under.
- **D-C6's precondition** — `ToggleTasksList` may leave `is_normal_only`
  **only** because no capital-letter spelling reaches it. If `Shift+T` is kept,
  it must stay `is_normal_only`, and the Insert-mode regression in exit
  condition 3 will fail.

## 5. Scope

**In scope**

1. **Consolidate.** `(CONTROL, Char('t'))` → `ToggleTasksList` in all three
   keymaps. Retire the `Shift+T` binding in **both** its `SHIFT` and `NONE`
   encodings, and drop `ToggleTasksList` from `is_normal_only`.
2. **`todo_expanded` (D-C7).** Decide and record: retire it, or keep it driven by
   the tasks toggle. Recommended: retire. `todo_strip_height` already returns 1
   whenever todos exist, so the always-visible one-line summary is unaffected;
   only in-place expansion is lost, and the full list remains the overlay's
   first group. If it is retired, `ToggleTodoList`, `toggle_todo_list`,
   `todo_expanded`, and `TuiMsg::ToggleTodoList` become dead and are removed
   **together** — a half-retired toggle is worse than either state.
3. **I6 guard.** Strengthen `documented_keys_resolve_in_the_active_map` to
   compare the resolved action against the advertised label.
4. **Help correctness.** Fix `src/tui/input.rs:1592`; document the Editor-route
   `Ctrl+T` and `Ctrl+S` overrides per **D-C8**.
5. **Orphan purge.** Wire `src/tui/components/help_overlay.rs` into
   `src/tui/components/mod.rs` **or** delete it. Default to delete — it
   duplicates `input.rs`'s table, and its only unique content (the Editor
   `Ctrl+S` note) belongs in the live table.

**Out of scope**

- Any new `Ctrl+S`, `Ctrl+J`, or `Shift+W` binding (D-C1/C2/C3 — withdrawn or
  settled).
- Moving the Editor's `Ctrl+T` file-tree toggle (D-C8 recommends documenting the
  route difference instead; changing the editor belongs to the editor roadmap).
- `/sessions` sections (C002), `/agents` (C003), chain coverage (C004),
  descriptors (C005), node lifetime (C006), interaction (C007).

## 6. Required production changes

**`src/tui/input.rs`**

- Three keymaps: `(CONTROL, Char('t'))` → `InputAction::ToggleTasksList`.
- Delete the `Shift+T` entries (SHIFT and NONE). Update the module doc table at
  `:27-30` and the help entries in both the Insert (`:1231`) and Normal
  (`:1589`) sections.
- Remove `ToggleTasksList` from `is_normal_only`, with a comment recording
  **why** it is safe (no capital spelling reaches the action) so a future
  re-added `Shift+T` cannot silently reintroduce the composer bug.
- Extend `documented_keys_resolve_in_the_active_map` (`:2765-2816`) to compare
  label↔action. If `HelpEntry.action` is free-form prose, introduce a canonical
  `InputAction` → label mapping and assert against that. **Do not** assert
  string-equality against a second hand-kept list — that relocates the same
  drift rather than removing it.
- Update `:2529` and `:2570` to the new mapping.

**`src/tui/app/mod.rs`, `types.rs`, `render.rs`**

- Remove or repurpose `toggle_todo_list` / `todo_expanded` per D-C7, together.
- `render.rs:1278` reads `todo_expanded`; reconcile it with the chosen answer.

**Docs**

- `docs/tui.md` — one key, three groups; note the Editor-route override.
- `architecture/tui.md` — same, **plus** the correction already recorded in the
  addendum: it still describes subagent rows as joined to durable runs by exact
  canonical task id, which describes the dead `u64`-vs-UUID join the code at
  `src/tui/app/render.rs:2613-2620` says was deleted.

**Not touched:** storage, protocol, scheduler, jobs, daemon, providers,
`docs/execution-ownership.toml`.

## 7. Ordered work packages

**WP-1 — Establish the I6 guard before touching help text.** Extend the guard and
run it. It should report the stale `Ctrl+T` row and any other latent drift.
Record the finding — it is the evidence this milestone exists. *Acceptance:*
the guard fails on an injected mismatch, proving it is not vacuous (the
`check_daemon_cwd_usage.py` precedent).

**WP-2 — Rebind and retire.** `Ctrl+T` → `ToggleTasksList` everywhere; delete
`Shift+T` in both encodings; drop the `is_normal_only` entry. Update `:2529`
and `:2570`. *Acceptance:* the combined block opens from Insert mode, and a
capital `T` still composes.

**WP-3 — Resolve D-C7.** Decide `todo_expanded`; if retiring, remove the toggle
path completely and in one change.

**WP-4 — Help correctness.** Fix `:1592`; document the Editor-route overrides;
cross-list `Ctrl+T` in the Normal help section now that it works in both modes.

**WP-5 — Orphan purge.** Wire or delete `help_overlay.rs`. *Acceptance:* every
module under `src/tui/components/` is declared in `mod.rs`.

**WP-6 — Docs.** `docs/tui.md`, `architecture/tui.md` (incl. the stale
`run_id`-join correction).

## 8. Failure, cancellation, restart, contention semantics

None new. No state, persistence, or async work is added.

One **layout** risk is real and inherited: `tasks_strip_height` and
`tasks_overlay_allowance` were sized for a Normal-mode on-demand block and are
now reachable from Insert, i.e. more often and mid-composition. The existing
clamps (`render.rs:1323-1342`, `MAX_TASKS_TOTAL_ROWS = 12`) already bound it, and
I4's transcript floor is asserted across 100x30, 100x50 and 120x24. Re-run those
assertions rather than assuming they still hold at the new reachability.

## 9. Compatibility and migration

No protocol, storage, or configuration change. **One user-visible behavior
change:** a user who learned `Shift+T` in the last two days loses it, and a user
who used `Ctrl+T` to expand the todo list in place now gets the combined block
instead. Both are recorded in the closure record; neither is a config or data
migration.

## 10. Required tests

- **Guard (WP-1):** label↔action comparison, plus an injected-mismatch proof.
- **Consolidation:** `Ctrl+T` resolves to `ToggleTasksList` in **both** modes.
- **Regression:** a capital `T` in Insert composes text — this is the test that
  fails if `Shift+T` were kept while `is_normal_only` was relaxed.
- **No dead binding:** no keymap resolves any chord to `ToggleTodoList` after
  D-C7 retires it.
- **Layout:** the existing transcript-floor assertions across three terminal
  sizes still pass.
- **No regressions:** `cargo test --test modifier_chords`,
  `cargo test --test tui_main_buffer`, `cargo test --test tui_side_panels`.

## 11. Required verification commands

```bash
scripts/verify.sh quick
cargo test --test modifier_chords
cargo test --test tui_main_buffer
cargo test --test tui_side_panels
python3 scripts/check_tui_transport_authority.py
cargo fmt
```

## 12. Documentation updates

- `docs/tui.md` — `Ctrl+T` = todos + subagents + jobs; empty groups hidden.
- `architecture/tui.md` — module map; **stale `run_id`-join correction**.
- `plans/registry.md` — register C001 `active` → `closing` → `closed` with its
  closure record.
- Reconcile the parent roadmap's `Status: proposed` header against the
  registry, per addendum D-8.

## 13. Acceptance criteria (externally observable)

1. From Insert mode — with no prior `Esc` — `Ctrl+T` opens a block listing every
   todo, live subagent, and live job, with a group heading only where that group
   has rows.
2. With nothing running and no todos, the block is empty and says so rather than
   showing three empty headings.
3. Typing a capital `T` in the composer types a `T`.
4. Every key advertised in `?` performs the action advertised beside it, in the
   mode it is listed under.
5. `Ctrl+S` still stashes the prompt (and still saves on the Editor route);
   `Shift+W` still opens the workspace dashboard; `Ctrl+J` is still unbound.

## 14. Stop conditions

Report rather than improvise if:

- Any proposal arises to satisfy a request by binding `Ctrl+S`, `Ctrl+J`, or
  `Shift+W`. D-C1/C2/C3 settled those; stop and re-open the decision.
- Retiring `Shift+T` cannot be done without also removing `ToggleTasksList`
  from `is_normal_only` — that coupling means some other capital spelling is
  still bound, and the implementer has not found it.
- D-C7 cannot be settled without changing the compact todo strip's always-visible
  one-row behavior. That is the parent's stated reason for keeping it and is not
  this milestone's to trade.
- The I6 guard cannot be made non-vacuous without restructuring `HelpEntry`.
- Anyone asks to move the Editor's `Ctrl+T` file-tree toggle. Per D-C8 that
  belongs to the editor roadmap.

## 15. Closure evidence required

A closure record at
`plans/closure/tui-subagent-and-job-visibility-convergence/001-status.md` with:
the requirement-to-evidence matrix; the WP-1 injected-mismatch proof; the drift
the guard found; the D-C7 decision and its rationale; the two user-visible
behavior changes in §9 stated plainly; and verification commands with truthful
local-vs-CI labels.

## 16. Handoff notes

**Start with WP-1, not WP-2.** Four of the original five requests were already
implemented and invisible. Running the corrected guard before editing help text
turns that claim into a measured number, and tells C002 whether the same drift
exists in the surfaces it is about to extend.

**Do not relax `is_normal_only` as a shortcut.** The tempting move is to leave
`Shift+T` bound *and* remove the gate, which silently breaks the composer for
any capital `T`. Exit condition 3 exists to catch exactly that; if it fails, the
fix is to finish retiring `Shift+T`, not to restore the gate and document it.