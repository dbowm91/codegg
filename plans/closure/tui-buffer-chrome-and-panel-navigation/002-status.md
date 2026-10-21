# TUI Buffer Chrome, Exit Protocol, and Files Panel Navigation M002 — Closure Status

Status: closed

Source implementation plan: none. This roadmap's milestones were implemented
from the roadmap's own deliverable boundaries and exit conditions, so the
roadmap **is** the plan and the closure record below is the only evidence
artefact it has.

Source subsystem roadmap:

- `plans/subsystems/tui-buffer-chrome-and-panel-navigation-roadmap.md` (M002)

Repository baseline reviewed: `a588a264` (branch
`feat/auto-20261020-513a8ce6`)

Applicable ADRs (unchanged by this milestone): none.

## 1. What landed

**The `/files` box can filter.** The Ctrl+T pane had a live filter, a panel
mode and a filter field; the box — the same `FilesDialog` component rendered
as a modal and as a docked panel — had none of them, which made the documented
"a fix in one is a fix in all three" parity false for the filter.

`i` now opens the panel's own insert mode in the box. Printable keys narrow
the rows, `Backspace` deletes, `Enter` applies, and `Esc` leaves insert mode
**keeping the filter**, so the rows the user narrowed to stay narrowed. The
filter is panel-local: the composer badge reports the app-level mode, and a
filter being typed is never reported as the composer being in Insert.

The filter narrows the **already-loaded** rows. `scripts/check_daemon_listing_authority.py`
puts `tui/components/dialogs/files.rs` and `tui/app/state/file_tree.rs` on its
protected list and rejects `read_dir(`, `build_tree(` and `local_walk_root(`
in them; `FileTreeState::rows()` satisfies that, and no walk is issued by a
filter.

A listing landing while the user is typing no longer wipes what they typed:
the mirror that copies the shared state into the box carries the box's `filter`
and `panel_mode` across.

**The bottom of the panel is quiet.** Two things were there:

- The box's own hotkey row, which duplicated the keymap the row above the
  composer already carries. It is gone — in the modal box as well as in the
  hosted panel, which had already dropped it.
- The truncation notice, which read `… stopped at the maximum depth of 3`
  **permanently**: any repository with a third level trips the walker's depth
  cap, so the notice was on screen at all times. The depth window is a
  navigational bound the panel slides around, not a loss of entries, so it is
  no longer reported as one. Every other bound — the node budget, the
  examined-entry budget, the wire budget — hides entries the panel genuinely
  cannot show, and those are still reported.

The depth reason is single-sourced: `directory_listing::depth_stop_reason()` and
`directory_listing::is_depth_stop_reason()` are imported by both the walker
and the TUI, so the two cannot drift into disagreeing about which bound fired.

## 2. Exit conditions

| Exit condition | Result | Evidence |
|---|---|---|
| `i` reaches insert mode in the `/files` box | met | `FilesDialog::handle_key` → `handle_insert_key` |
| `Esc` keeps the filter | met | `handle_insert_key` — the filter is not cleared |
| the filter narrows loaded rows only | met | no walk is issued; `FileTreeState::rows()` filters |
| a listing completion does not wipe a filter being typed | met | `commands/file_tree.rs` — the mirror carries `filter` and `panel_mode` |
| the depth window is not reported as a truncation | met | `FileTreeState::apply_listing` filters the depth reason |

## 3. What was verified

- `cargo nextest run --workspace --locked --profile ci --no-fail-fast` —
  **15,606 tests run: 15,601 passed, 4 failed, 1 timed out, 7 skipped.** Every
  test this milestone touched passes. The five non-passing ones are the
  environment's, not this change's: the two `core::daemon_refresh` projection
  tests fail with `InvalidRootPath("/tmp")` on the untouched base commit too,
  `agent::asset_refresh::tests::same_scope_requests_coalesce_to_one_publication`
  is the 300 s coalescing wait that the machine's load turns into a timeout,
  and the two `tui::commands::interactive_terminal` PTY-attach tests are
  load-flaky — three consecutive runs of that module alone gave 37/37, 36/37
  and 37/37 with a *different* test failing each time, and both fail before any
  key is routed.
- `cargo nextest run -E 'binary(codegg) and (test(file_tree) or test(panels::tests))'`
  — 139/139 green.
- `cargo fmt --all -- --check` — green.
- The twelve TUI guards (`check_tui_editor_*`, `check_tui_composer_key_authority`,
  `check_tui_transport_authority`, `check_daemon_listing_authority`) — green.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — green.
- `scripts/verify.sh quick` — green.

## 4. Recorded deviations

- The `/files` box's filter is **box-local**: it lives in the box's copy of
  the shared state and does not survive the box being closed, while the
  Ctrl+T pane's filter lives in the app's shared state and does. The request
  was that INS mode filters the rows and that `Esc` keeps the filter; both
  hold. Sharing one filter across the two surfaces would mean the pane's
  filter and the box's filter are the same string, which is a different
  contract from the one that was asked for.
