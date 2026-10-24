# File Picker Rework Post-Closure Corrective Milestone M001 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/file-picker-rework-post-closure-corrective/001-one-authority-for-the-navigator-tree.md`
Source subsystem roadmap: `plans/subsystems/file-picker-rework-post-closure-corrective-addendum.md`
Repository baseline reviewed: `6e097dd3a` (`Merge pull request #180 from dbowm91/fix/repository-audit-corrective-implementation`, 2026-10-23)
Implementation commits:

- `c24147fbc` — fix(tui): make the /files navigator's tree one authority
- `6d368c9fb` — chore(tui): trim the touched files' comments and re-bank the budget
- `180a4ec3b` — docs(tui): record the /files navigator's ownership split
- `745286bb4` — test(tui): prove the /files filter stays box-local
- `d8173cf6b` — test(tui): prove the page verbs move the authority's selection

## 1. Executive finding

The `/files` navigator had **two** copies of its tree state and only **one** of them was
reachable. The push-back that reconciles them lived in exactly one place — inside
`apply_listing`, gated on `focus_manager.has_dialog(DialogType::Files)` — and a docked
panel's widget is owned by `SidePanelHost`, not the focus manager, so the gate was false
for the whole time the panel was docked. The panel rendered a frozen copy: `l` expanded a
directory in an authority the panel never rendered, `h` collapsed nothing, `r` repainted
nothing, and `j`/`k` moved a cursor the authority never saw.

A second, independent defect had the same root: `ascend_one_level`'s `Leave` and
`Collapse` arms mutate the authority **without** a re-walk, so no listing lands, so the
mirror never runs — which made the **modal box's** `h` inert as well as the panel's.

M001 makes `App::file_tree_state` the single owner of the navigator's tree **structure**,
replaces the widget's mutable clone with a read-only view of it, turns every verb into a
path-bearing intent resolved against the authority, and refreshes every mounted
surface's view from the authority **once per frame, before that surface is drawn**,
gated on a revision counter. The docked panel and the modal box now navigate
identically.

## 2. Requirement-to-evidence matrix

| # | Acceptance criterion (plan §13) | Evidence |
|---|---|---|
| 1 | A docked Files panel expands a directory with `l`, collapses it with `h`, enters it with `Enter` and repaints it with `r`, each observable in the panel | `tests/tui_render/file_picker_surface_corrective.inc` — `a_docked_files_panel_expands_collapses_and_enters_through_the_real_key_path`, `the_docked_panel_repaints_after_r_refreshes_the_authority` |
| 2 | The modal box's `h` collapses an entered directory, observably | `the_modal_boxs_h_collapses_an_entered_directory` |
| 3 | The widget holds no mutable copy of the navigator's tree structure, and its view type cannot hold one | Type-level: `FileTreeStructure` (`src/tui/app/state/file_tree.rs`) holds no authority state, and the widget's only structural field is `view` (`src/tui/components/dialogs/files.rs`). Behaviourally: `the_refresh_copies_only_when_the_revision_moved`, `the_docked_panel_is_painted_from_the_authority_not_from_a_frozen_copy` |
| 4 | The selection names a path, so it does not depend on a filter or on two copies agreeing about an index | `the_docked_panels_cursor_moves_with_j_and_k`, `the_selection_names_a_path_and_an_unknown_path_is_refused`, `the_two_surfaces_show_the_same_cursor_and_the_same_rows`; `FileTreeState::peek_up(&self, path)` resolves against the unfiltered row set |
| 5 | The `/files` box's filter is still box-local, and the Ctrl+T pane's is still in the authority | `the_box_filter_is_box_local_and_the_pane_filter_stays_in_the_authority`; unit `the_filter_is_the_boxs_own_and_never_the_authoritys`; commit `745286bb4` |
| 6 | The refresh copies the authority only when it changed | Unit `the_refresh_copies_only_when_the_revision_moved`: 0 copies on an unchanged revision, 1 on a moved one |
| 7 | The tree still never reads the filesystem, and the daemon listing protocol is unchanged | `scripts/check_daemon_listing_authority.py` green; no listing, walker or listing-DTO file appears in the diff |
| 8 | `scripts/verify.sh quick` is green and workspace clippy is clean | See §4 |

## 3. Production implementation evidence

### 3.1 The ownership split, as implemented

**The authority — `App::file_tree_state`** (`src/tui/app/state/file_tree.rs`): the tree
structure `nodes`, `expanded`, `selected`, `root`, `root_history`, `listing_prefix`,
`pending_cursor`; the Ctrl+T pane's own state `filter`, `panel_mode`, `scroll`,
`viewport_rows`; and the `revision` counter that makes the refresh total and cheap.

**The read-only projection — `FileTreeStructure`**, built by exactly one function,
`FileTreeState::structure()`. It carries `rows`, `selected`, `root`, `scope`,
`truncated`, `status` — and nothing the widget could mutate.

**The surface — `FilesDialog`** (`src/tui/components/dialogs/files.rs`): a `view` of
type `FileTreeStructure`, a `view_revision`, and the genuinely per-surface `filter`
(box-local), `panel_mode`, `scroll` and `visible_height`.

### 3.2 The verbs are path-bearing intents

`TuiMsg::FilesDescend` / `FilesAscend` / `FilesSelect` (`src/tui/app/types.rs`),
dispatched in `src/tui/app/input.rs`, applied against the authority in
`src/tui/commands/file_tree.rs`. `l` already carried a path; `h` and the movement verbs
now do too, so "the parent of this row" is resolved **structurally** against the
unfiltered row set rather than against a possibly-stale selection.

### 3.3 The refresh reaches every mounted surface

`App::refresh_file_tree_surfaces` (`src/tui/app/render.rs`) is called at the top of the
frame, ahead of the tree pane, the side panel and the modal dialog. It reaches the
modal box through `FocusManager::with_dialog_mut` and the **docked panel** through the
new `SidePanelHost::with_hosted_mut` → `HostPanel::with_widget_mut` seam. The copy
happens only when the revision moved.

The old mirror — the `apply_listing` copy gated on `has_dialog(DialogType::Files)` — is
**gone**. `FilesDialog::apply_listing`, which had no production caller, was removed;
mounting the box now adopts the structure through the *same* mechanism the frame pass
uses, so there is one way to adopt a listing.

## 4. Verification executed

**All of it was run locally. No hosted CI lane has run against these commits.**

| Command | Result |
|---|---|
| `bash scripts/verify.sh quick` | exit 0 — "==> Quick verification passed." |
| `CARGO_BUILD_JOBS=3 cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0 — 0 warnings |
| `cargo test --test tui_render -- file_picker_surface_corrective` | 10 passed |
| `cargo test --lib tui::app::state::file_tree` | 47 passed |
| `cargo test --lib tui::components::dialogs::files` | 25 passed |
| `cargo test --lib tui::commands::file_tree` | 23 passed |
| `cargo test --lib tui::commands::editor::tests::space_f` | 3 passed |
| `cargo fmt --all --check` | clean |
| `python3 scripts/rust_test_scan.py --validate` | pass |
| `python3 scripts/check_test_assertion_quality.py` | pass |
| `python3 scripts/check_test_duplication.py` | pass |
| `python3 scripts/check_daemon_listing_authority.py` | pass |
| `python3 scripts/check_comment_budget.py` | pass |
| `python3 scripts/check_test_target_references.py` | pass |

The whole root `cargo test --lib` suite was also run: **7,433 passed, 5 failed**. Four of
the five are **pre-existing** and were reproduced on baseline `6e097dd3a`: two
`core::daemon_refresh` tests failing on `InvalidRootPath("/tmp")`, one
`managed_process` test failing on `OfflineRequired` being unsupported on this host, and
one flaky `interactive_terminal` badge test that passes in isolation. The fifth
(`editor::tests::space_f…`) **was** caused by this change — the box's title is read
before the frame pass has adopted a structure — and was fixed by adopting the structure
at mount time.

## 5. Invariant review

| Invariant | Result |
|---|---|
| I1 — one authority for the tree structure | **met** — `FileTreeStructure` is a distinct read-only type; the widget's only structural field is a view |
| I2 — a view never writes the authority | **met** — the view has no mutating accessor and is rebuilt from the authority |
| I3 — the box's filter stays box-local | **met** — proven by `745286bb4` |
| I4 — the tree never reads the filesystem | **met** — the daemon-listing-authority guard is green; the listing, walker and DTO are untouched |
| I5 — the listing round-trip stays generation-guarded | **met** — a stale completion is still dropped, and it does not bump the revision |
| I6 — render cost does not regress | **met** — 0 copies on an unchanged frame, 1 per mounted surface on a changed frame; the TUI render-cost guard is green |
| I7 — `Esc` remains the mode switch and never an exit | **met** — unchanged |
| I8 — the tree pane's keymap, chrome and behaviour are unchanged | **met** — the tree pane still mutates the authority directly; only its writes now go through the revision-bumping setters |
| I9 — `Enter` and `l` remain one motion | **met** — unchanged |

## 6. Failure and recovery review

- A stale listing completion is dropped by the existing generation guard and does not
  bump the revision, so no surface is refreshed with a listing that lost the race.
- A listing that lands while a surface is being typed into does not discard the
  in-progress filter input, because the refresh replaces only the structural fields.
- A surface unmounted between the refresh and the draw is simply not reached; the
  refresh tolerates its absence.
- A failed listing keeps the fail-visible behaviour and bumps the revision so the
  surface shows the error.

## 7. Migration and compatibility review

- No persistence, no migration, no `STORAGE_LAYOUT_VERSION` move.
- No protocol change, so no daemon compatibility question.
- No configuration change.
- The `/files` box's filter stays box-local, so the two surfaces' row sets still
  differ. That is the accepted contract (§14.1 of the roadmap), and this milestone did
  not change it.

## 8. Security review

Nothing changed that touches authority, authorization, or path resolution. No new read
surface was introduced and the TUI still never walks the filesystem itself: the daemon
remains the only reader, and `scripts/check_daemon_listing_authority.py` is green. No
listing, walker or listing-DTO file appears in the diff.

## 9. Documentation and operations

- `architecture/tui.md` and `docs/tui.md` record the ownership split (`180a4ec3b`).
- `architecture/overview.md`'s enforced integration-test file count was re-measured
  334 → 335 by `python3 scripts/measure_overview_counts.py`, and the reconciliation is
  recorded in `architecture/count_audit_history.md`.
- The comment-budget ratchet was re-banked twice and holds **down**: the global ceiling
  moved 122,472 → 122,455 lines and 1,078,679 → 1,078,132 words against baseline. The
  ratchet forbids growth and may be lowered freely, so keeping it down required trimming
  pre-existing prose in the touched files — including part of the C004 single-flight
  rationale in `src/tui/commands/file_tree.rs`.

## 10. Unresolved findings

- **medium — the hosted CI lane has not run against these commits.** Every gate above
  is local. The named re-entry condition is a green `CI / verify` run on the milestone's
  final head. This is recorded against the milestone, not against the implementation.
- **low — the `peek_up` behaviour change.** `peek_up` now resolves against the
  unfiltered row set (plan §6), so with the Ctrl+T pane's own filter active, `h` finds
  the parent even when that filter hides it. `h` was previously dead in that state, so
  this is a fix rather than a regression; it is recorded because the visible rows and
  the ascended row can now differ.
- **low — plan §10 deviation.** The "the refresh reaches a panel-host-owned surface"
  unit test is proven by the integration tests instead: the seam is typed, and
  `a_surface_unmounted_before_the_refresh_is_not_reached` shows a foreign surface is not
  reached. The behaviour is proven; the proof is at a different level than the plan
  asked for.
- **No critical or high finding remains.**

## 11. Roadmap disposition

M001 is **closed**. The roadmap's §7 M001 exit conditions are all met: the docked panel
and the modal box navigate identically, the box's `h` is observable, the widget holds no
mutable copy of the navigation structure, the box's filter is still box-local, and the
refresh costs one copy per changed authority per frame and none on an unchanged frame.

No closed decision was reopened — see the roadmap's §14 and the plan's §16.

## 12. Registry updates

- `plans/registry.md`: the `File picker rework post-closure corrective` subsystem row
  moves to M001 closed; the M001 implementation-plan row moves to **closed** with this
  record as its closure.
- M002 remains **ready for handoff**, now unblocked: its hard dependency was M001.
- **Unblock audit:** M001's only hard dependent was M002, which is now registered as
  ready. No other registered plan lists M001 as a hard or interface dependency, and no
  previously blocked plan had M001 as its blocker. Nothing else became newly eligible.
