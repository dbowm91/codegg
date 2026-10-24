# File Picker Rework Post-Closure Corrective Milestone M001 — One authority for the navigator's tree

Status: implemented

> **Closed.** Outcome and verification are recorded in
> `plans/closure/file-picker-rework-post-closure-corrective/001-status.md` — read
> that first. The one named condition on the closure is that the hosted CI lane has
> not yet run against these commits; every gate in the closure record is local.

Repository baseline: `6e097dd3a` (`Merge pull request #180 from dbowm91/fix/repository-audit-corrective-implementation`, 2026-10-23)

Source roadmap:

- `plans/subsystems/file-picker-rework-post-closure-corrective-addendum.md` — §5 target architecture, §7 M001, §10 risks and decisions, §14 closed decisions

Long-term requirements:

- `plans/000-long-term-specification.md` — TUI-first product; correctness before transparent magic.
- `plans/001-terminology-and-domain-model.md` — workspace-relative path identity; document attachment.
- `plans/002-long-term-roadmap.md` — TUI evolution on frontend-neutral projections.

Applicable ADRs:

- ADR-0022 (daemon-authorized workspace directory listing) — consumed unchanged.
- ADR-0033 (directory-tied project identity) — consumed unchanged.

Primary class: invariant (with a capability consequence)

Corrective plan. It corrects `plans/implementation/file-picker-rework/001-panel-keymap-authority-and-listing-hierarchy.md`
and its closure record `plans/closure/file-picker-rework/001-status.md`, which closed the
*keymap and chrome* contract but not the *state-ownership* contract beneath it.

## 1. Objective

Make `App::file_tree_state` the single owner of the navigator's tree structure, turn
every `/files` widget verb into a path-bearing intent resolved against that owner, and
refresh each mounted surface's view from it before that surface is drawn — so the docked
Files panel and the modal box navigate identically, and the `h` verb is observable in
both.

## 2. Why this milestone is ready

- **Hard dependencies**: none. The daemon listing, the bounded walk and the worker
  permit closed under `plans/subsystems/tui-editor-correctness-usability-roadmap.md`
  (M002, C007, C008); the panel keymap and chrome contract closed under
  `plans/subsystems/file-picker-rework-roadmap.md` M001 (`b4f4b916f`); the navigator's
  verbs, filter and chrome closed under
  `plans/subsystems/tui-buffer-chrome-and-panel-navigation-roadmap.md` M001–M004.
- **Interface dependencies are stable and in-tree**: `Component`, `SidePanel`,
  `HostPanel`, `FocusManager`, `FileTreeState`, `TuiMsg`.
- **No unresolved architectural decision.** No ADR is required: no identity, ownership
  boundary, protocol, authorization, persistence or compatibility contract changes.
  What changes is which copy of an existing *frontend* value is authoritative.
- **No closed decision is reopened** — see §16.

## 3. Current implementation evidence

Baseline `6e097dd3a`. Paths below are repository-relative; line numbers are the
baseline's and are evidence, not a contract.

### 3.1 Two copies; only one is reachable

`FileTreeState` is declared in `src/tui/app/state/file_tree.rs` and holds `visible`,
`root`, `expanded`, `selected`, `scroll`, `nodes`, `truncated`, `status`, `request`,
`generation`, `cancel_flag`, `root_history`, `listing_prefix`, `viewport_rows`,
`panel_mode`, `filter`, `pending_cursor` and `pending_editor_handoff`.

`App` owns one as `file_tree_state`. The `/files` widget
(`src/tui/components/dialogs/files.rs`) owns a second as `pub state: FileTreeState`, a
clone taken in `open_file_picker` (`src/tui/app/modal.rs`).

The clone is pushed back into the widget in exactly one place — the mirror inside
`apply_listing` (`src/tui/commands/file_tree.rs`) — and that mirror is gated on
`app.focus_manager.has_dialog(DialogType::Files)`.

A docked panel's widget is owned by `SidePanelHost`, not the focus manager:
`dock_open_box` (`src/tui/commands/panels.rs`) calls `close_dialog()` (which unmounts
the widget from the modal stack) and adopts it into the panel. The gate is false for a
docked panel, so a docked panel's widget holds a **frozen** copy for as long as it is
docked, and the panel renders that copy.

### 3.2 Which fields are written from which side

- **Authority-side writes** (production): `selected` — `commands/file_tree.rs` and
  `app/state/file_tree.rs`; `expanded` — `app/state/file_tree.rs` only; `filter` —
  `commands/file_tree.rs` only (the tree pane's filter verbs); `panel_mode` —
  `commands/file_tree.rs` only (the tree pane's insert mode); `pending_cursor`,
  `root_history`, `listing_prefix`, `nodes` — `app/state/file_tree.rs` only.
- **Widget-side writes** (on the clone): `selected` — the widget's `move_selection`,
  `page_selection`, `reveal` and `handle_insert_key`; `filter` — the widget's
  `handle_insert_key`; `panel_mode` — the widget's `handle_insert_key` and its `i` arm.

So `expanded`, `pending_cursor`, `root_history`, `listing_prefix` and `nodes` are
already authority-only in production. `selected` is the field with two writers that
matters for this milestone.

`FilesDialog::apply_listing` is the only place the widget writes `expanded` or `nodes`,
and it has **no production caller** — the mirror replaces `dialog.state` wholesale
instead.

### 3.3 The mirror preserves what the widget owns, by hand

The mirror saves the widget's `filter` and `panel_mode`, assigns `dialog.state =
snapshot`, then restores them. That is the code acknowledging the two-copy design
rather than fixing it.

### 3.4 Row builders: filtered and unfiltered

- `all_rows()` — unfiltered, built from `nodes` and `expanded`.
- `rows()` — **filtered** by `filter`, case-insensitively on the name or the path.
- `selected_index`, `selected_row` and `peek_up` read **filtered** rows.
- `peek_into` reads **unfiltered** rows.

`peek_up` and `peek_into` disagreeing about the filter is the asymmetry that makes `h`
resolve against the wrong row set.

### 3.5 The tree pane is the reference implementation

The Ctrl+T tree pane's handler (`src/tui/commands/file_tree.rs`) mutates
`app.file_tree_state` directly for every verb — `move_selection`, `page_selection`,
`peek_into_selected`, `ascend_one_level`, `activate_selected`, `rebuild`, and the
filter's push/pop — and the app renders that pane from the same value. The `/files`
widget is the only surface that keeps a second mutable copy.

### 3.6 Two fields that a read-only view would break

- `reveal()` writes the **scroll** offset into the widget's copy. The widget already
  owns a separate `pub scroll: CenteredScroll` field, so the scroll is duplicated
  inside `FileTreeState` and inside the widget.
- `viewport_rows` — the row count a page step covers — is written back into the
  **authority** during the tree pane's render (`src/tui/app/render.rs`), and the
  `/files` widget's own `page_selection` writes it into its copy. Two surfaces, one
  field, and it is mutated during a render pass.

### 3.7 Where a refresh can happen

`App::render` (`src/tui/app/render.rs`) is the only frame path. It draws the tree pane,
then the side panel, then the modal dialog, so **one** insertion point ahead of all
three refreshes every mounted surface.

### 3.8 Prior art

`research/tui-files-panel-depth-and-input-chrome-2026-10-21.md` §1.1 already recorded
the surface map and that the parity claim in `architecture/tui.md` *"is false for the
filter and false for `Enter`"*. The `Enter` half was closed by
`tui-buffer-chrome-and-panel-navigation` M001; the filter half was left as a recorded
deviation; the **state-ownership** half is not closed by any milestone. This milestone
is that corrective, and it cites the research rather than claiming the discovery.

## 4. Invariants that must not regress

- **I1 — One authority.** The navigator's tree structure — nodes, expansion set, scope
  and selection — is owned by exactly one value. A surface may hold a read-only view of
  it; it may not hold a second mutable copy that can disagree.
- **I2 — A view never writes the authority.** Drawing a surface must not change what
  the other surfaces see.
- **I3 — The box's filter stays box-local.** A closed decision — see §16. The tree
  pane's filter stays in the authority.
- **I4 — The tree never reads the filesystem.** The daemon remains the only reader.
  `scripts/check_daemon_listing_authority.py` must stay green.
- **I5 — The listing round-trip stays generation-guarded.** A stale completion is
  dropped, not painted.
- **I6 — Render cost does not regress.** `scripts/check_tui_render_cost.py` must stay
  green: the refresh must not introduce a per-frame copy of the whole tree.
- **I7 — `Esc` remains the mode switch and never an exit.**
- **I8 — The tree pane's own keymap, chrome and behaviour are unchanged.**
- **I9 — `Enter` and `l` remain one motion** in the navigator.

## 5. Scope

### In scope

- The `/files` widget's ownership of the navigator's tree structure.
- The `/files` widget's verbs: `j`, `k`, `Ctrl+d`, `Ctrl+u`, `l`, `h`, `Enter`, `r`,
  `i`, the filter's printable keys, `Backspace` and `Esc`.
- The authority's view refresh for every mounted `/files` surface.
- The `selected`, `scroll` and `viewport_rows` ownership split.

### Explicitly out of scope

- The `q` exit verb and the editor handoff — **M002**.
- The **filter's ownership** — it stays box-local (§16.1).
- The tree pane's keymap, chrome, rendering and filter behaviour.
- The daemon listing protocol, the walk, the worker permit and the listing DTO.
- The filter's semantics — substring match on name or path, live narrowing, `Esc` keeps
  the filter.
- `Ctrl+B`, `Shift+Q`, `q` and every other key's meaning.
- The editor.

## 6. Required production changes

The implementation agent may adjust file-level mechanics; the ownership outcomes below
are the contract.

### Core/domain

- `App::file_tree_state` remains the owner of `nodes`, `expanded`, `selected`,
  `root`/`root_history`, `listing_prefix` and `pending_cursor`.
- `FileTreeState` gains a **revision counter**, bumped by every mutation that changes
  what a surface would render. The counter is what makes the refresh cheap and total.
- `scroll` and `viewport_rows` are resolved so that each surface pages against **its
  own** viewport height, and the tree pane's render-time write into the authority does
  not invalidate a surface's view.
- `peek_up` resolves the selected path against the **unfiltered** row set, the way
  `peek_into` already does. "The parent of this row" is a structural question, not a
  filtered one.

### Protocol and DTOs

None.

### Runtime and concurrency

- The listing round-trip, its generation/request-id discipline and its stale-completion
  drop are unchanged. The revision counter rides alongside them.

### Frontend surface

- `FilesDialog` stops owning a mutable `FileTreeState`. It holds a **read-only view**
  of the authority's structure for rendering, plus the genuinely per-surface state: the
  filter text, the insert mode, and the scroll offset.
- The view is a **distinct type** from the authority, so it cannot hold an authoritative
  field it might mutate. That makes I1 structural rather than conventional. Its
  construction is one function, and it carries no behaviour that duplicates
  `FileTreeState`'s — the existing free row-builders are reused.
- Every verb becomes a path-bearing `TuiMsg` intent handled by `commands::file_tree`
  against the authority. `l` already carries a path; `h` and the movement verbs become
  path-bearing. The existing `FilesDescend`, `FilesAscend`, `FilesOpenSelected` and
  `FilesRefresh` keep their meaning.
- The authority's view is pushed into every mounted `/files` surface **once per frame,
  before those surfaces are drawn**, and only when the revision changed. The insertion
  point is in `App::render`, ahead of the tree pane, the side panel and the modal
  dialog.
- The refresh reaches **both** the modal dialog and the docked panel. It must not be
  gated on the modal stack.
- The widget's filter, insert mode and scroll are **preserved** across the refresh; the
  refresh replaces only the structural fields.
- `FilesDialog::apply_listing` is removed, or becomes the refresh's mechanism — but it
  must not remain a second, dead way to adopt a listing.

### Security and authorization

None. No new read surface, no new path resolution, no new authority.

### Documentation and static guards

- `architecture/tui.md` and `docs/tui.md` state the ownership split truthfully.
- A static guard asserts that the navigator's published keymap and the widget's hint
  describe one set of verbs (M002 owns the keymap content; the guard lands with it).

## 7. Ordered work packages

### Work package A — Decide the ownership split in code terms

Intent: decide which of `selected`, `filter`, `panel_mode`, `scroll` and
`viewport_rows` the authority owns and which the surface owns.

Required changes: none on their own; this package produces the decision the following
packages implement. The decision must satisfy all of:

- the selection names a **path**, so it does not depend on a filter or an index;
- a surface's paging uses **its own** viewport height;
- an unfocused surface shows no caret and no filter caret;
- a surface's in-progress filter typing is not lost by a listing landing;
- the filter stays **box-local** (§16.1).

Acceptance evidence: the decision is recorded as a comment at the authority's
declaration and at the widget's declaration, and it names every field.

### Work package B — Give the authority a revision counter

Intent: make the refresh cheap and total.

Required changes: a revision counter on `FileTreeState`, bumped by every mutation that
changes the rendered result — including the selection, the expansion set, the scope and
the tree pane's render-time viewport write.

Acceptance evidence: a focused test asserts that a mutation bumps the revision, that
an unrelated mutation does not, and that a viewport-height write during a render bumps
it too.

### Work package C — Introduce the read-only view

Intent: make I1 structural.

Required changes: the widget's view type, built by one function from the authority,
carrying the structural fields and nothing else; the widget keeps the filter, the
insert mode and the scroll in its own state.

Acceptance evidence: a focused test asserts that the view carries no field the widget
could mutate, and that the view's rows are the authority's rows for the same filter.

### Work package D — Make every verb path-bearing

Intent: remove the widget's second cursor.

Required changes: `h` and the movement verbs stop resolving "where am I" against a
possibly-stale selection and stop mutating the widget's copy; they name a path, and the
command layer applies it to the authority. `peek_up` resolves the named path against
the unfiltered row set.

Acceptance evidence: a focused test asserts that pressing `j` in the box moves the
**authority's** selection and that `h` ascends from the row the user is on, not from a
stale one.

### Work package E — Refresh every mounted surface, at render time

Intent: make the refresh reach the docked panel as well as the modal box, and make it
total.

Required changes: the refresh is driven from the app's frame pass, ahead of the
surfaces' draw, and reaches a widget the `SidePanelHost` owns. The app needs one
type-checked seam for that — modelled on `FocusManager::with_dialog_mut`, using the
`AsAny` bound that already exists for components. The refresh copies only when the
revision changed, and preserves the surface's filter, insert mode and scroll.

Acceptance evidence: an integration test docks `/files` into the panel and asserts that
the panel's rendered rows change after a `l` on a directory; a focused test asserts
that an unchanged authority costs no copy.

### Work package F — Remove the dead listing-adoption path

Intent: one way to adopt a listing.

Required changes: `FilesDialog::apply_listing` is removed, or becomes the refresh's
mechanism.

Acceptance evidence: no production caller of the removed path remains, and the guards
`scripts/check_daemon_listing_authority.py` and `scripts/check_tui_render_cost.py` stay
green.

## 8. Failure, cancellation, restart and contention semantics

- A **stale listing completion** is dropped by the existing generation guard, and the
  revision is not bumped for it, so no surface is refreshed with a listing that lost the
  race.
- A **listing that lands while a surface is being typed into** must not discard the
  in-progress filter input. The refresh preserves the surface's filter, and the
  implementation must prove this rather than assume it.
- A **surface that is unmounted** between the refresh and the draw is not reached; the
  refresh must tolerate a surface that is not present.
- A **failed listing** keeps the existing fail-visible behaviour and bumps the
  revision so the surface shows the error.
- There is **no concurrency**: the TUI is single-threaded and the refresh happens on
  the frame path.

## 9. Compatibility and migration

- No persistence, no migration, no `STORAGE_LAYOUT_VERSION` move.
- No protocol change, so no daemon compatibility question.
- No configuration change.
- The `/files` box's filter stays box-local, so **the two surfaces' row sets still
  differ** — the accepted contract (§16.1). This milestone does not change that, and
  makes the selection path-bearing precisely so that coherence does not depend on the
  two agreeing about a filter.

## 10. Required tests

### Focused unit tests

- Every authority mutation that changes the rendered result bumps the revision; an
  unrelated mutation does not; a viewport-height write during a render does.
- The refresh reaches a surface owned by the panel host, not only a modal dialog.
- The refresh preserves the surface's filter, insert mode and scroll.
- The refresh copies only when the revision changed.
- `j`/`k`/`Ctrl+d`/`Ctrl+u` move the **authority's** selection.
- `h` ascends from the row the user is on.
- The filter's printable keys, `Backspace` and `Esc` update the surface's filter and
  re-anchor its selection.
- Paging uses the surface's own viewport height.
- A listing landing does not discard in-progress filter input.

### Integration tests

- Dock `/files` into the side panel and assert, through the real key path: `l` on a
  directory expands it in the panel; `h` collapses it; `Enter` enters it; `r` repaints
  it; `j`/`k` move the panel's cursor.
- The same verbs in the modal box assert the same observable results.
- The two surfaces show the same cursor and the same rows after the same key sequence,
  with no filter active in either.

### Restart and recovery tests

- None. No durable state changes.

### Contention and cancellation tests

- A stale listing completion does not refresh a surface.
- A surface unmounted before the refresh is not reached.

### Security and negative tests

- The tree still never reads the filesystem (the daemon-listing-authority guard is the
  test).
- A path that the authority does not know still resolves to no action, silently, as
  today.

### Migration and compatibility tests

- None.

## 11. Required verification commands

```bash
# narrow tests first
cargo test --test tui_render -- files_tree_authority
cargo test --test tui_render -- files_panel_navigation
cargo test -p codegg --lib tui::commands::file_tree
cargo test -p codegg --lib tui::components::dialogs::files

# static guards
python3 scripts/check_daemon_listing_authority.py
python3 scripts/check_tui_render_cost.py --self-test
python3 scripts/rust_test_scan.py --validate

# formatting and linting
cargo fmt --all
CARGO_BUILD_JOBS=3 cargo clippy --workspace --all-targets --locked -- -D warnings

# the canonical gate
bash scripts/verify.sh quick
```

Do not claim commands that were not actually run in the closure record. The workspace
test sweep and `scripts/verify.sh full` are **not** required for this milestone; the
narrow targets above plus `quick` are the gate, and the hosted CI lane is recorded
separately.

## 12. Documentation updates

- `architecture/tui.md` — the `/files` section: the ownership split, and the corrected
  statement of what the three surfaces share.
- `docs/tui.md` — the browsing-files section, if it describes the surfaces' behaviour.
- `architecture/overview.md` — only if the Verified-counts table changes (re-measure
  with `python3 scripts/measure_overview_counts.py` and record the reconciliation in
  `architecture/count_audit_history.md`).

## 13. Acceptance criteria

1. A docked Files panel expands a directory with `l`, collapses it with `h`, enters it
   with `Enter` and repaints it with `r`, each observable in the panel.
2. The modal box's `h` collapses an entered directory, observably.
3. The `/files` widget holds no mutable copy of the navigator's tree structure, and its
   view type cannot hold one.
4. The selection names a path, so it does not depend on a filter or on two copies
   agreeing about an index.
5. The `/files` box's filter is still box-local, and the Ctrl+T pane's is still in the
   authority.
6. The refresh copies the authority only when it changed.
7. The tree still never reads the filesystem, and the daemon listing protocol is
   unchanged.
8. `scripts/verify.sh quick` is green and workspace clippy is clean.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- the ownership split cannot be settled without changing the tree pane's keymap or
  chrome (out of scope — report it);
- the selection cannot be made path-bearing without changing the tree pane's behaviour;
- the **filter's ownership** would have to change — that is §16.1 and is closed;
- a refresh point that covers every mounted surface cannot be found without changing
  the render pass's structure beyond a local insertion;
- the revision counter would have to cover a field that a render pass mutates on every
  frame, which would make the refresh per-frame;
- the daemon listing protocol, the worker permit or the listing DTO would have to
  change;
- the change would expand into the editor, the tree pane or the panel host's chrome.

## 15. Closure evidence required

- The acceptance criteria, each with the test or command that proves it.
- The measured cost of the refresh: the number of copies per frame on an unchanged
  frame and on a changed frame.
- The list of fields the authority owns and the list the surface owns, as implemented.
- Evidence that the box's filter is still box-local and that the tree pane's is still
  in the authority.
- `scripts/verify.sh quick` and the clippy result, labelled local or hosted truthfully.
- The unblock audit: whether any registered plan's blocker is now discharged.

## 16. Handoff notes — closed decisions this milestone must not reopen

### 16.1 The box's filter is box-local

`plans/closure/tui-buffer-chrome-and-panel-navigation/002-status.md` §4 records it as a
**deviation**: *"The `/files` box's filter is box-local: it lives in the box's copy of
the shared state and does not survive the box being closed, while the Ctrl+T pane's
filter lives in the app's shared state and does. … Sharing one filter across the two
surfaces would mean the pane's filter and the box's filter are the same string, which
is a different contract from the one that was asked for."*

Keep it. Do not unify the filters. If the implementation appears to need one filter,
that is the signal to make the selection path-bearing instead — which is what this plan
requires.

### 16.2 The navigator's other closed contracts

- `Enter` and `l` are one motion (`tui-buffer-chrome-and-panel-navigation` M001). I9
  preserves it.
- `Esc` is the mode switch and never an exit; `Shift+Q` is the panel exit; `q` is the
  program's quit key (`tui-buffer-chrome-and-panel-navigation` M004). Unchanged here.
- The panel's keymap and chrome contract (`file-picker-rework` M001) is unchanged.

### 16.3 Operational

- **Baseline**: `6e097dd3a` on `fix/file-picker-surface-corrective`. The branch is
  based on `origin/main` **after** PR #180 merged, so the C001–C007 corrective
  workstream's counts, guards and comment budget are already in the baseline.
- **Resource constraint**: this machine has 14 cores and a shared disk. Use
  `CARGO_BUILD_JOBS=3` and run the narrow test targets, not the workspace sweep.
- **The narrowest correct test target**: the defect lives in the seam between two
  surfaces, so a unit test of either alone cannot see it. The integration family
  member is the load-bearing evidence.
- **Comment budget**: `scripts/check_comment_budget.py` is a ratchet that forbids
  growth. If the change adds comments, the budget must be lowered in the same commit,
  and the re-banking must follow `cargo fmt --all`.
- **Do not** reopen the tree pane's keymap, the panel host's chrome, or the editor.
