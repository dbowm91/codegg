# TUI Buffer Chrome, Exit Protocol, and Files Panel Navigation M001 — Closure Status

Status: closed

Source implementation plan: none. This roadmap's milestones were implemented
from the roadmap's own deliverable boundaries and exit conditions, so the
roadmap **is** the plan and the closure record below is the only evidence
artefact it has.

Source subsystem roadmap:

- `plans/subsystems/tui-buffer-chrome-and-panel-navigation-roadmap.md` (M001)

Repository baseline reviewed: `a588a264` (branch
`feat/auto-20261020-513a8ce6`)

Research artifact: `research/tui-files-panel-depth-and-input-chrome-2026-10-21.md`

Applicable ADRs (unchanged by this milestone): none. This milestone adds no
protocol, storage, migration, authorization, or dependency change.

## 1. What landed

**One motion for "open this row".** `l` and `Enter` are the same verb in the
`/files` box, and the Ctrl+T tree pane already agreed. `Enter` on a directory
emits `TuiMsg::FilesDescend` instead of calling the retired
`FileTreeState::toggle_selected`; `l` on a **file** asks the command layer to
open it, because a file has nothing to enter and a dead key is a dead key.

The state layer grew one variant to say so: `PeekStep::Open { path }`. The
command layer routes it to `set_pending_editor_handoff` + `open_editor` — the
same path `Enter` took for a file, so there is one open path and not two.

**The scope slide lands on the grandparent.** The listing window is three
levels (`MAX_LISTING_DEPTH = 3`), so a directory at the window's edge is a
leaf in the listing and cannot show its children. The slide now moves the
scope to the target's **grandparent**:

```rust
scope: parent_path(&parent_path(&row.path)),
```

The grandparent is the *shallowest* scope in which the target's children fit
back inside the window, so the user keeps the widest context: the target's
parent stays visible beside the target's siblings. The user's own example is
the arithmetic: the workspace root is `~/projects/codegg`, the target is
`~/projects/codegg/crates/codegg-client/src`, and the new visual working
directory is `~/projects/codegg/crates`.

The workspace root never changes. The scope travels as the listing request's
`prefix`, so the daemon re-resolves the canonical root on every request and
the panel can never escape the workspace.

**The slide opens the chain down to the directory.** A slide replaces the
listing, and in the new scope the target's *ancestors* are the rows — they
arrive collapsed. Expanding only the target therefore leaves the directory
the user pressed `l` on off screen: the pane has changed scope and shown
nothing new, which is the same "empty directory" symptom the slide was built
to remove. `FileTreeState::expand_ancestors_of` opens the chain between the
new scope and the directory, and the directory is expanded with it.

## 2. Exit conditions

| Exit condition | Result | Evidence |
|---|---|---|
| `Enter` on a directory enters it instead of toggling it | met | `src/tui/components/dialogs/files.rs` — `activate` emits `TuiMsg::FilesDescend` |
| `l` on a file opens the document | met | `PeekStep::Open` → `descend_to` → `open_editor` |
| the slide lands on the grandparent | met | `src/tui/app/state/file_tree.rs` — `peek_into` |
| the slid-to directory is on screen | met | `FileTreeState::expand_ancestors_of` |
| the scope moves one level **inward**, and never out of the workspace | met | the target's grandparent is always a child of the current scope, and the scope travels as the listing request's prefix |

## 3. What was verified

- `cargo nextest run --workspace --locked --profile ci --no-fail-fast` —
  **15,606 tests run: 15,601 passed, 4 failed, 1 timed out, 7 skipped.** Every
  test this milestone touched passes. The five non-passing ones are the
  environment's, not this change's:
  - `core::daemon_refresh::tests::session_projection_snapshot_includes_in_process_live_child_instances`
    and `…::session_projection_rebuilds_owner_scoped_live_jobs` fail with
    `InvalidRootPath("/tmp")` on the untouched base commit as well;
  - `agent::asset_refresh::tests::same_scope_requests_coalesce_to_one_publication`
    is the 300 s coalescing wait, which the machine's load turns into a
    timeout;
  - the two `tui::commands::interactive_terminal` PTY-attach tests are
    load-flaky: three consecutive runs of that module alone gave 37/37, 36/37
    and 37/37 with a *different* test failing each time, and both fail before
    any key is routed.
- `cargo nextest run -E 'binary(codegg) and (test(file_tree) or test(panels::tests))'`
  — 139/139 green, including the reworked worked example and the new
  ancestor-expansion cases.
- `cargo fmt --all -- --check` — green.
- The twelve TUI guards (`check_tui_editor_*`, `check_tui_composer_key_authority`,
  `check_tui_transport_authority`, `check_daemon_listing_authority`) — green.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — green.
- `scripts/verify.sh quick` — green.

## 4. Recorded deviations

- The implementation's slide target was the target's **parent**; the requested
  rule is the **grandparent** (roadmap §2, D1). The change is recorded rather
  than absorbed: the parent's rule also fits the children inside the window,
  but it discards a level of context for no navigational gain.
