# File Picker Rework Milestone 001 — Closure Status

Status: conditionally closed

Conditionally, and for one reason: **the condition is a commit SHA at which
`tui-panel-surface-convergence-corrective` M001 can re-inspect
`src/tui/components/component.rs` and `src/tui/panels/hosted.rs`.** That
condition is met by `b4f4b916f7bd0fed85863419fab838cb9ca9f4c4` (§7.7 records
the merge it was verified on). Every acceptance criterion in the plan is met
and verified; nothing else here is outstanding.

Source implementation plan:

- `plans/implementation/file-picker-rework/001-panel-keymap-authority-and-listing-hierarchy.md`

Source subsystem roadmap:

- `plans/subsystems/file-picker-rework-roadmap.md`

Repository baseline reviewed: `b77c8156b594494b7f59e0348d93a73ca2c1fd1c`

Implementation commits or pull requests:

- Landed on `main` as **`b4f4b916f7bd0fed85863419fab838cb9ca9f4c4`**
  (`feat(tui): panel owns its keymap, host owns its chrome`), on branch
  `feat/auto-20261016-9ebb3f33`
  (worktree `.worktrees/feat-auto-20261016-9ebb3f33`), fast-forwarded onto
  `origin/main` `5a7381432` — that commit rewrites the working strip, the
  tasks overlay and `docs/tui.md`/`architecture/tui.md`, three of the files
  below; the merge was automatic, with no conflict markers, and both sides'
  assertions survive it (§7.7).
- `tui-panel-surface-convergence-corrective` M001 re-inspects `component.rs`
  and `hosted.rs` at that SHA.

## 1. Executive finding

The milestone's capability boundary is complete. A `/files` box docked in a side
panel now (a) shows no keymap inside the column, (b) shows its real keymap on the
status line above the composer **whether or not** it holds the keyboard, and (c)
draws a tree guide that visibly steps in one level for every component of the
scope it was entered through. The host's border, title and `●` focus marker
survive the widget, so a focused panel is once again distinguishable from an
unfocused one.

Nothing navigational changed. `Enter` still expands in place, `l`/`→` still
re-roots, `h`/`←` still ascends, and the walk, protocol, storage and auth
surfaces are byte-identical.

One **plan defect** was found and had to be resolved rather than implemented
literally, and one pre-existing bug was found in passing. Both are in §7.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Docked panel paints no keymap in the column | `FilesDialog::render_in_panel`; `a_hosted_render_keeps_the_hosts_chrome_and_spends_every_row_on_rows` asserts `!text.contains("enter open")` at 30×8 | pass | The modal keeps its footer: `the_modal_box_keeps_its_own_footer` |
| Panel keymap always on the status line, focused or not | `a_docked_files_panel_advertises_its_own_keymap_without_the_keyboard` (unfocused) and `a_focused_panel_names_the_way_back_to_the_composer` | pass | The headline defect |
| The keymap is the box's own, not a host paraphrase | `the_keymap_the_host_advertises_is_this_box_keymap` asserts `keymap_hint() == HINT`; the panel test asserts `!hint.contains("i act")` | pass | `Component::keymap_hint`, written beside `handle_key` |
| The entry gesture is placement-correct | `an_unfocused_panel_names_the_placement_correct_entry_gesture` — Right `Shift+→/Shift+L`, Left `Shift+←/Shift+H`, Bottom `Shift+↓/Shift+J` | pass | From `PanelPlacement::toward` + `PanelDirection::gesture_key`, the same values the dispatcher parses |
| No other hosted panel's line changed | `every_other_hosted_panel_keeps_its_original_hint_line` — byte-identical `"<Title> · j/k scroll · i act · Shift+← back"` for every non-Files hosted id | pass | The generic string survives as the `keymap_hint() == None` fallback |
| An empty panel says nothing and takes no focus | `an_unloaded_panel_is_quiet_and_refuses_the_keyboard` | pass | Behavior unchanged; the old `"empty · run /files to load"` branch was **provably unreachable** — see §7.2 |
| Host title and focus dot survive | `a_hosted_widget_does_not_paint_over_the_hosts_title` asserts `● Files` on row 0 after `render_hosted` | pass | |
| Scoped listings visibly step in | `a_scoped_listing_draws_its_rows_one_level_in` — buffer assertion, row for `top.txt` renders `│  └─ …` inside a listing scoped to `dir`, and further right than the same row in the root listing | pass | Baseline-failing; this is the acceptance signal |
| `display_depth` adds the scope back | `display_depth_adds_back_the_scope_a_subtree_walk_restarted` (table), `display_depth_clamps_at_the_one_cap_both_renderers_share` | pass | |
| Depth is a read, never navigation | `display_depth_is_a_read_and_changes_nothing_about_navigation` — rows, selection, expansion and scroll all unchanged after reading depth on every row | pass | |
| One guide function, two callers, one cap | `the_guide_ladder_is_one_function_for_both_renderers`, `a_deep_scope_is_clamped_to_one_cap_rather_than_unbounded`; both renderers call `file_icons::tree_indent`; the duplicate `MAX_INDENT_DEPTH` consts are deleted | pass | |
| The guide needs no Nerd Font | `the_guide_is_box_drawing_and_needs_no_nerd_font` — `│ ├ ─ └` are BMP and outside the PUA | pass | Deliberately **not** behind `CODEGG_FILE_ICONS` |
| Narrow columns elide the guide, never the name | `a_narrow_column_elides_the_guide_before_the_name` — for every depth 0..12 × budget 0..24, guide width ≤ budget | pass | |
| Scope named as a breadcrumb | `the_title_is_a_breadcrumb_of_the_scope` — `Files · <workspace> › dir › nested (0 shown)` | pass | |
| Existing empty/unbound hint tests pass **unmodified** | `tests/tui_chat_surfaces.rs` ran unmodified in the 217-test sweep | pass | |
| `Enter`/`l` split unchanged | `enter_expands_a_directory_in_place_without_losing_the_cursor`, `l_asks_the_command_layer_to_root_the_listing_at_the_directory`, `h_asks_the_command_layer_to_go_back_one_level` — all pre-existing, all pass | pass | |
| No key, `..` row, or typeahead added | `git diff --stat` — no new `KeyCode` arm in either renderer or in `handle_key` | pass | |
| Walk / protocol / storage / auth untouched | `git diff --name-only` matches no `directory_listing`, `crates/`, migration, schema or auth path | pass | |

## 3. Production implementation evidence

`git diff --stat` against `b77c8156`:

```text
 .opencode/skills/tui/SKILL.md       |  26 +++
 architecture/tui.md                 |  96 ++++++++++-
 docs/tui.md                         |  41 ++++-
 plans/registry.md                   |   6 +
 src/tui/app/mod.rs                  |   9 +-
 src/tui/app/state/file_tree.rs      | 166 ++++++++++++++++++
 src/tui/components/component.rs     |  23 +++
 src/tui/components/dialogs/files.rs | 336 ++++++++++++++++++++++++++++++++++--
 src/tui/components/file_tree.rs     |  28 +--
 src/tui/file_icons.rs               | 207 +++++++++++++++++++++-
 src/tui/panels/hosted.rs            |  47 +++--
 src/tui/panels/mod.rs               | 304 ++++++++++++++++++++++++++++++--
 12 files changed, 1222 insertions(+), 67 deletions(-)
```

### `src/tui/components/component.rs` — two defaulted trait methods

`keymap_hint() -> Option<String>` (default `None`) and
`render_in_panel(&mut Frame, Rect, &Arc<Theme>)` (default delegates to
`render`). Both are **defaulted**, so the ~50 existing `Component` implementors
and every other hosted panel are untouched. Making either required would have
touched every implementor for no reason.

### `src/tui/components/dialogs/files.rs` — the box owns its verbs and its rows

`keymap_hint()` returns `HINT` verbatim — the same `pub const` the modal footer
paints, sitting in the same file as the `handle_key` arms that implement it.
`render_in_panel()` paints the listing into `area.inner(Margin::new(1,1))` and
nothing else: no border, no title, no footer. `render()` is otherwise unchanged
in shape, so the modal box still owns its whole surface. `title()` became a
breadcrumb built from the workspace name plus each `/`-separated component of
`root_prefix()`.

### `src/tui/panels/hosted.rs` — the host asks the widget

`composer_hints` returns `None` for a panel with no adopted box, the widget's
`keymap_hint()` (or `GENERIC_KEYS`) for a focused panel with
`Shift+← back`, and the same keys prefixed with the placement's entry gesture
for an unfocused one.

### `src/tui/panels/mod.rs` — honest focus state, and the host keeps its chrome

`PanelHints` gained `placement: PanelPlacement` plus `enter_gesture()` /
`exit_gesture()`; `PanelDirection::gesture_key()` renders a direction as
`Shift+→/Shift+L` from the same `gesture()` letter the dispatcher parses.
`SidePanelHost::composer_hints` no longer returns early when unfocused — it
passes `focused: self.is_focused()` and dispatches either way.
`render_hosted` now calls `widget.render_in_panel(frame, frame.area(), theme)`.

### `src/tui/app/state/file_tree.rs` + `src/tui/file_icons.rs` — one depth answer

`display_depth` = `scope_depth() + row.depth`, clamped at the single
`MAX_INDENT_DEPTH` (moved here from two private copies). `indent_for` pairs depth
with last-ness for `└─` over `├─`. `file_icons::tree_indent` renders both and
sizes them to the column, eliding levels from the outside in.

## 4. Verification executed

### Commands run

```bash
cargo fmt --check
cargo ck
cargo clippy --workspace --all-targets --locked -- -D warnings   # -D warnings
scripts/verify.sh quick
scripts/verify.sh full
cargo test -p codegg --lib tui::
cargo test -p codegg --lib tui::components::dialogs::files
cargo test -p codegg --lib tui::panels
cargo test -p codegg --lib tui::file_icons
cargo test --test tui_panel_docking --test tui_usability_fixes \
           --test tui_chat_surfaces --test tui_render
CARGO_BUILD_JOBS=2 cargo build --release --locked
```

All local. **No CI run was made for this change**, and the registry's other rows
cite hosted CI numbers; this row cites none.

### Results

| Check | Result |
|---|---|
| `cargo fmt --check` | pass |
| `cargo ck` (workspace check) | pass |
| `cargo clippy --workspace --all-targets -D warnings` | pass (after one fix; see §7.3) |
| `scripts/verify.sh quick` | **pass** — every boundary/ownership/route/audit/scheduler/config/provider guard plus `cargo check --workspace --all-targets` |
| `cargo test -p codegg --lib tui::` | **1864 passed, 1 failed** — the failure is pre-existing, see §7.1 |
| `cargo test --test tui_panel_docking --test tui_usability_fixes --test tui_chat_surfaces --test tui_render` | **217 passed, 0 failed** (29 + 52 + 111 + 25) |
| `scripts/verify.sh full` | **does not pass in this environment** — see §7.1. Not caused by this change. |

`verify.sh quick` passing is the meaningful gate here: it is a **superset** of
the CI job, which per `AGENTS.md` is a strict subset of `quick`. `full` adds
Clippy (which passes) and the whole workspace sweep, which aborts on unrelated
failures before reaching most of the tree.

## 5. Invariant review

- `FilesDialog` still holds no file text and performs no reads. `r` is still an
  intent; the walk is still the daemon's.
- `Enter`-expands / `l`-re-roots / `h`-ascends is untouched, and re-rooting is
  still a daemon round-trip rather than a filter of the loaded listing.
- The listing still arrives already walked, and `walk_directory` is unchanged.
- `display_depth` and `indent_for` are reads. They touch no expansion, no
  selection, no scroll, and nothing a walk returns — asserted directly.
- Depth is clamped in exactly one place and the clamp is shared, so the two
  renderers cannot disagree about it.
- The composer hint row is still carved from the **prompt** allocation, not the
  transcript: `hint_rows = u16::from(composer_hint_text().is_some())` is
  unchanged, so neither composer input height nor the transcript's `Min(10)`
  floor moved.
- Key-ownership precedence in `App::on_key` is unchanged.
- No protocol, storage, migration, identity or authorization surface was touched.

## 6. Failure and recovery review

Not applicable in the usual sense — this milestone adds no async work, no new
request, and no new state that can fail. The two seams added to `Component` are
synchronous and infallible. `render_in_panel` returns early on a zero-area rect
rather than drawing into one.

## 7. Unresolved findings

### 7.1 `verify.sh full` does not pass here — three pre-existing failures

None is caused by M001, and each was checked by stashing the M001 diff and
re-running:

| Test | At baseline (M001 stashed) | With M001 | Cause |
|---|---|---|---|
| `core::daemon::tests::session_create_through_daemon` | **FAILED** | not reached | Environmental: `InvalidRootPath("/tmp")` — this machine's daemon refuses `/tmp` as a workspace root |
| `agent::asset_refresh::tests::same_scope_requests_coalesce_to_one_publication` | passed | **passed** in isolation | Load-sensitive: nextest `ci` profile, 4 concurrent, 120 s timeout under full-sweep load |
| `tui::commands::interactive_terminal::tests::pressing_i_on_the_open_box_enters_focused_input` | **FAILED** | **FAILED** | Pre-existing in this worktree; asserts a freshly opened terminal box is not focused |

The sweep aborts on the first failure, so it stopped at **1110 / 14167 tests**
(1108 passed, 1 failed, 1 timed out, 6 skipped) and 13057 tests did not run.
That is the honest extent of `full` coverage here — this record does not claim a
green workspace sweep.

### 7.2 The plan's §6.4 mechanism was wrong, and was resolved differently

The plan specified gating the always-on hint on `!panel_is_empty()` alone.
Taken literally that **fails the plan's own §10 requirement**: the session panel
is the pre-docked default column and `panel_is_empty()` returns `false` for it
unconditionally, so a fresh app would show a session key line under the composer
on every frame of every session — a permanent line nobody asked for, which
`tests/tui_chat_surfaces.rs::no_panel_focused_advertises_no_keys` forbids.

Resolved by letting **each panel** decide what is honest to say without the
keyboard, which is what the panels were already written for — `TerminalPanel`
had an unfocused branch that the host's gate made dead code. Session, chat, help
and agents decline; a panel the user *docked* speaks. Both protected tests pass
unmodified.

The consequence, recorded so the next reader does not "restore" it: **the session
panel stays silent until you go into it.** If the intent was "every panel,
always", that is a different rule and it would need the session/chat/help/agents
branches changed too.

### 7.3 Two dead branches removed rather than ported

- `HostPanel::composer_hints`'s `"empty · run {cmd} to load"` line could never
  render: `focus()` refuses an empty panel and `is_focused()` requires
  `!panel_is_empty()`, so `hints.focused` is never `true` in that branch. It is
  replaced by `None`, and `Placeholder::render` remains the one place that names
  the command.
- `MAX_INDENT_DEPTH` was duplicated in two renderers with the same value and the
  same rationale; the copies are deleted.

### 7.4 Plan evidence corrected after reading the walker

The plan (and the registry row at the time) stated a scoped listing's rows
"arrive at `depth == 0`". They do not: `walk` emits `depth + 1`, so the top level
of a subtree walk arrives at **the same depth the workspace root's own top level
arrives at**. The defect and the fix are unchanged — the two are the same number,
which is why they rendered identically — but every comment, doc and test now
says the true thing. `docs/validation/` line numbers are pinned to old SHAs by
design and were not refreshed.

### 7.5 An unrelated WIP edit does not compile, and was not folded in

The worktree carried an uncommitted `src/tui/editor.rs` change calling
`SelectionState::select_range`, **which does not exist**. The tree therefore did
not compile at baseline. Per the plan's handoff note it was not folded into this
milestone; it was stashed for the duration and **restored** afterwards. It is
currently held in `stash@{0}` (`editor-wip-tmp-for-verify`) so the committed
tree builds; pop it to get the WIP back. Consequences:

- Every result above was produced **without** that change.
- Restoring it puts the tree back into a non-compiling state until
  `select_range` is added to `src/tui/helix_selection.rs`.
- It is **not** what makes `pressing_i_on_the_open_box_enters_focused_input`
  fail: that failure reproduces on a pristine `origin/main` worktree with none
  of this milestone's changes present (§7.6).

This is unowned work outside M001's boundary. It needs either finishing or
dropping by whoever wrote it.

### 7.6 The one failing unit test is not this milestone's

`cargo test -p codegg --lib tui::` on the committed tree: **1866 passed, 1
failed** — `tui::commands::interactive_terminal::tests::pressing_i_on_the_open_box_enters_focused_input`,
panicking on `a freshly opened box starts viewing, never focused`.

That failure is **pre-existing**. It reproduces on a pristine `origin/main`
worktree created at `5a7381432` with none of M001's changes present
(`git worktree add --detach`, separate target dir, single-test filter):
`0 passed; 1 failed`. M001 therefore neither causes nor fixes it. Nothing in
this milestone touches `interactive_terminal`.

### 7.7 Verification on the merged tree

`origin/main` advanced during implementation (`5a7381432`, "three status
surfaces that silently read as nothing happened"), which rewrites the working
strip, the tasks overlay, and `docs/tui.md` / `architecture/tui.md` — three
files M001 also edits. The branch was **fast-forwarded onto it first**, then
M001's work restored: the merge was automatic, produced no conflict markers,
and both sides' assertions survive (main's `awaiting_task_counts`,
`bounded_task_rows().len().max(1)`, the `Nothing pending` doc line; M001's
`keymap_hint`, `render_in_panel`, `tree_indent`, `GENERIC_KEYS`, `PanelPlacement`,
`MAX_INDENT_DEPTH`, breadcrumb, and the `composer_hint_text` note). Main's
`render.rs` hunk touches no panel-hint or composer-hint code.

Re-verified after the merge:

- `scripts/verify.sh quick` — **pass** (full boundary/guard set plus
  `cargo check --workspace --all-targets --locked`).
- `cargo test -p codegg --lib tui::` — 1866 passed, 1 pre-existing failure (§7.6).
- `tests/tui_panel_docking.rs` 52, `tests/tui_render.rs` 111,
  `tests/tui_chat_surfaces.rs` 29, `tests/tui_usability_fixes.rs` 25,
  `tests/tui_main_buffer.rs` 44 (main's own new suite) — **261 passed,
  0 failed**. The two hint tests that `tui_usability_fixes.rs` requires to pass
  unmodified still pass.

## 8. Security review

No authorization, capability, identity or trust-boundary change. The hint line
and the tree guide are display-only; the guide renders row names the listing
already contained, adds no path the walk did not produce, and adds no user input
to a terminal escape sequence (all glyphs are fixed BMP constants).

## 9. Documentation and operations

`docs/tui.md` — "Browsing files" gains "Where you are, three ways" (breadcrumb +
guide + `CODEGG_FILE_ICONS` note) and "Where the keys are" (the status line, the
placement-correct entry gesture, the modal-vs-panel split); "Moving between the
box and the panel" cross-references it. `architecture/tui.md` — the `/files`
section gains "Depth is display arithmetic"; the panel section gains the
host-owns-the-chrome rule and the widget-owned keymap rule, and the "Contextual
keys above the input" entry now states the unfocused rule and which panels
decline. `.opencode/skills/tui/SKILL.md` updated with both contracts, per
`AGENTS.md`'s rule that a module contract change moves the skill and the
`architecture/` doc together. `architecture/overview.md` needed no change — no
module was added. `## Verified counts` needed no change.

## 10. Unblock audit

- `tui-panel-surface-convergence-corrective` M001 was registered **blocked**
  pending this milestone's closure, specifically so it can re-inspect
  `component.rs` and `hosted.rs` at a commit. **That condition is now met** —
  this record ships in that commit, and its registry row records the SHA and is
  updated from *Blocked* to *Unblocked*. That plan should proceed on the two
  seams as landed — `keymap_hint() -> Option<String>` and
  `render_in_panel(&mut Frame, Rect, &Arc<Theme>)`, both defaulted.
  Note for that plan: `Placeholder` is itself a chrome-overwriting renderer and
  an unfocusable panel, so it inherits both halves of this change's reasoning.
- `file-picker-rework` M002 stays **not approved**. No candidate was opened, and
  nothing here unblocks one — typeahead, a `..` row and the narrow-column guide
  policy remain out of scope and untouched.
- No other registered plan changes dependency state.

## 11. Residual risk

- **Appearance, not semantics.** The tree guide changes how *every* row of every
  file listing draws, including rows at the workspace root, which now get a
  connector they did not have. That is the intended tree look, but it is a
  visible change beyond the reported defect and has not been seen by a human
  yet.
- **The unfocused-panel line is long.** `Files · Shift+→/Shift+L in · j/k move ·
  ^d/^u page · l/→ root at dir · h/← up · enter open · r refresh · esc close`
  is ~110 columns and will be ellipsized in a narrow terminal. The two most
  important facts (`enter open`, `l/→ root at dir`) sit in the middle and may be
  what disappears. Worth one look on a real terminal; shortening `HINT` was not
  in scope and would change the modal footer too.
- The session/chat/help/agents panels advertise nothing until entered (§7.2). If
  that reads as too quiet, the fix is in those four `composer_hints` arms, not
  here.