# Desktop Frontend and IDE Foundation Milestone 006-D — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/desktop-frontend-ide-foundation/006-project-scoped-file-tree.md`

Source subsystem roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#milestone-006--tui-ide-vertical-slice`

Repository baseline reviewed: `d776dbb2` (plan merge); fix head `de29e49e`

Implementation commits / pull requests:

- `c2c65bd7` — bounded tree model, view state, widget, and wiring.
- `80363a64` — architecture documentation and the tree-to-open trajectory.
- PR `#94`, merged as `de29e49e`.
- Plan PR `#93`, merged as `d776dbb2`.

## 1. Executive finding

M006-D's capability boundary is complete: the TUI can browse a project-scoped
workspace tree and open any file in it through the existing controller-backed
editor path. The load-bearing property is that the tree is a **navigator, not a
second content path** — it enumerates directory entries, never file bytes, and
delegates every open to the existing `open_editor`, so document text remains
solely owned by the M005 controller and the M006-A text-authority guard stays
green and non-vacuous with an **unchanged** `SCANNED_FILES` list. The milestone
added no protocol, storage, migration, or authorization surface and required no
ADR, because it makes no ownership or delivery decision: its root is the active
tab's explicit `ProjectExecutionContext::workspace_root`, resolved exactly as
`open_editor` already resolves it.

## 2. Requirement-to-evidence matrix

| Requirement (plan §4/§13) | Evidence | Result | Notes |
|---|---|---|---|
| Tree root is the active tab's explicit `workspace_root`; no ambient cwd or path-as-identity | `rebuild` calls `app.project_execution_context()` and uses `context.workspace_root`; `check_tui_project_authority.py` green | pass | Authority is inherited from `open_editor`'s existing pattern, not decided in-pass |
| Editor still obtains all text through the controller; guard green and non-vacuous with an unchanged `SCANNED_FILES` | `git show --stat 80363a64` shows `scripts/check_tui_editor_text_authority.py` untouched; guard green locally and hosted | pass | This PR does not modify the guard at all |
| Symlinks never followed at any depth; a symlink loop terminates | `symlinks_are_never_followed_at_any_depth`, `symlink_loop_terminates`, and the integration `a_symlinked_file_is_not_offered_as_an_openable_document` | pass | Checked on the raw entry, because `canonicalize` resolves symlinks |
| Bounded on node count, depth, and name length; truncation displayed not silent | `node_cap_truncates_and_says_so`, `depth_cap_truncates_and_says_so`, `truncation_notice_survives_into_state`, `a_truncation_notice_is_rendered` | pass | Depth 12, 4096 entries, 255-byte names |
| Every tree-derived path is workspace-relative and `..`-free | `paths_are_workspace_relative_and_dotdot_free`, plus trajectory assertions on the selected row | pass | Paths are built from validated component names |
| Opening from the tree matches `/open` | `a_file_discovered_by_the_tree_opens_like_any_typed_path`; `activate_selected` calls `open_editor` verbatim | pass | Controller reports `Synced`; transport records the exact path |
| `Route::Editor` stays non-modal; composer editable | Tree keys are a strict subset claimed only while focused; `Esc` leaves the tree without a route change; `default_is_hidden_and_empty_so_m006a_surface_is_untouched` | pass | 1079 `tui::` tests, including all 1045 pre-existing M006-A tests, pass unchanged |
| Pane degrades rather than degenerating on a narrow terminal | `pane_splits_only_when_there_is_room` | pass | Falls back to the full-width editor |
| Unreadable directory does not blank the tree | `unreadable_directory_does_not_blank_the_tree` | pass | Skipped, not fatal |
| Stale walk completions are discarded | `a_stale_generation_completion_is_discarded`, `a_completion_for_another_root_is_discarded`, `reset_for_project_invalidates_an_in_flight_walk` | pass | Guarded by request id, generation, and root |
| fmt, clippy, guards, `verify.sh quick` | all green, below | pass | — |
| Hosted `CI / verify` completes the full sweep | run `37232046785` | pass | 12205/12205, sweep completed |

## 3. Production implementation evidence

- `src/tui/file_tree.rs` — the bounded walk, `TreeNode`, `TreeListing`, and
  `visible_rows`. Caps depth at 12, total entries at 4096, and entry names at
  255 bytes; records which bound stopped the walk; skips symlinks on the raw
  entry; skips an ignore list mirroring the composer's `@` indexer; skips
  unreadable directories rather than failing; sorts directories before files
  then lexically.
- `src/tui/app/state/file_tree.rs` — `FileTreeState` and `FileTreeStatus`.
  Hidden by `Default`; carries root, expansion set, selection, scroll,
  truncation notice, and request/generation tracking. `apply_listing` and
  `apply_error` both refuse a completion whose request id, generation, or root
  does not match. A rebuild for a different project clears selection and
  expansion, and prunes expansions for directories that no longer exist.
- `src/tui/components/file_tree.rs` — the widget and `split_pane`, which
  degrades to the full-width editor below `MIN_TREE_PANE_WIDTH * 2`. Only rows
  that fit the pane are materialized.
- `src/tui/commands/file_tree.rs` — `toggle_tree`, `rebuild`, `apply_listing`,
  `move_selection`, `toggle_selected`, `activate_selected`, `set_focus`. The
  walk runs on a scoped `TuiTaskKind::FileTree` task, or inline when there is no
  command channel so the pane cannot be left stuck in `Loading`.
- `src/tui/commands/editor.rs` — `handle_file_tree_key` claims `j`/`k`, `h`/`l`,
  `Enter`, `r`, and `Esc` only while the tree is focused and returns `false`
  otherwise; `Ctrl-T` toggles the pane from anywhere in the editor.
- `src/tui/app/render.rs` — `render_editor_view` splits the viewport and calls
  `render_file_tree_pane`, which borrows only tree state and the theme. The
  editor's controller snapshot is still borrowed and dropped inside its own
  render, so the frame holds no document buffer in either configuration.
- `src/tui/app/commands.rs`, `src/tui/runtime/command_dispatch.rs`,
  `src/tui/task_lifecycle.rs` — the `FileTreeListed` completion, its dispatch
  arm, and the `TuiTaskKind::FileTree` variant.
- `src/tui/app/state/ui.rs` — `tree_focused`, a separate flag from the
  editor's buffer/composer focus so the non-modal contract stays explicit.

No production change was made to `codegg-core`, `codegg-protocol`,
`codegg-client`, or any crate outside the root `codegg` package.

## 4. Verification executed

### Commands run

```bash
cargo test --lib tui::                       # 1079 passed
cargo test --test file_tree_trajectory       # 4 passed
cargo fmt --all -- --check                   # clean
cargo clippy --workspace --all-targets --locked -- -D warnings   # clean
python3 scripts/check_tui_project_authority.py       # passed
python3 scripts/check_tui_editor_text_authority.py   # passed
python3 scripts/check_execution_ownership.py         # ok
scripts/verify.sh quick                              # exit 0
```

### Results

Local:

- `cargo test --lib tui::`: **1079 passed, 0 failed**. That is 34 new tests
  over the 1045 present after M006-A, with every pre-existing M006-A test still
  passing and the pane hidden by default.
- `cargo test --test file_tree_trajectory`: **4 passed, 0 failed**.
- `cargo fmt --all -- --check`: clean.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: clean.
  Four findings were raised and fixed during implementation: an elidable
  lifetime on `visible_rows`, two `field_reassign_with_default` in tests, and
  a `needless_borrow` on `Path::join`.
- All three TUI/ownership guards green.
- `scripts/verify.sh quick`: exit 0.

Hosted (`CI / verify`, PR `#94`, head `80363a64`) — run **`37232046785`**, verify
job **success**, all twenty guard and lint steps green:

- `Workspace tests`: **`12205` tests run, `12205` passed, `7` skipped**, in
  398.405s. The sweep completed rather than being truncated by fail-fast.
- Every M006-D test passed, including the twelve
  `tui::app::state::file_tree::tests`, the eight
  `tui::components::file_tree::tests`, the fourteen `tui::file_tree::tests`,
  and the four `file_tree_trajectory` integration tests.

Three test-authoring mistakes were made and corrected during implementation;
they are recorded because they are the kind of thing that would otherwise read
as product behavior:

1. An expansion test asserted a grandchild was visible when only its parent was
   expanded. Expansion is per-directory, not recursive; the test was wrong.
2. A "long name is skipped" test tried to create a 256-byte filename. macOS
   `NAME_MAX` is 255, so the filesystem rejects it before the walk sees it. The
   255-byte cap is therefore *defensive rather than reachable* on common
   filesystems. The test now says so explicitly, gates on what is actually
   assertable, and the unconditional invariant — a shown name is always the real
   on-disk name, never a truncated prefix — is asserted separately.
3. A row assertion conflated "visible because the parent is expanded" with "this
   row is expanded". Different properties; both are now asserted separately.

## 5. Invariant review

- Document text authority is intact. The tree has no API for reading a file,
  and `open_editor` is the only route from a path to an attachment. The
  text-authority guard is green and non-vacuous, and its `SCANNED_FILES` list is
  byte-identical — confirmed by `git show --stat 80363a64`, which does not
  touch the guard.
- Project authority is intact. The root comes from the explicit execution
  context; `check_tui_project_authority.py` is green.
- `Route::Editor` remains non-modal. The composer stays editable, and the tree
  claims a strict subset of keys only while focused, so it cannot swallow
  composer or editor input. `Esc` leaves the tree without changing the route.
- `/open <workspace-relative path>` is unchanged, including its lexical
  validation and already-open short-circuit.
- The M006-A presentation record, reconciliation rules, bounded undo/redo, hard
  wrap, and async discipline are untouched; all 1045 pre-existing `tui::` tests
  pass.
- No production code outside `src/tui/` changed, and no crate in
  `crates/` changed.
- No protocol, storage, migration, configuration, or authorization change; no
  version-skew surface, because no wire surface changed. That is the concrete
  reason M006-D is separable from the ADR-gated M006-B.
- No historical closure record was edited. The causal-frontier and
  project-catalog corrective records and the M006-A `005-status.md` are
  untouched.

## 6. Failure and recovery review

- **Unreadable or concurrently removed directory:** skipped; the rest of the
  tree still renders.
- **Missing or unreadable workspace root:** the pane shows an explicit error
  state and the editor remains fully usable. The tree is never a precondition
  for the editor, which is why it renders even when no document is open.
- **Truncation:** a first-class displayed outcome, not a silent partial view.
- **Stale completion:** discarded by request id, generation, and root, so a
  walk for a workspace the user has left is never rendered.
- **Project switch:** root, expansion, and selection reset, because a path from
  the previous project is meaningless in the next.
- **No command channel:** the walk runs inline rather than leaving the pane
  stuck in `Loading` forever.
- **Hung test:** the `ci` nextest profile terminates any test after two 60 s
  slow periods, so the `Barrier(8)` cannot stall the suite even in the
  worst case. This was checked explicitly because the barrier's known hazard is
  that a task panicking before reaching it would block its peers.
- **Degenerate pane:** the layout split falls back to the full-width editor
  rather than rendering a one-column tree.

## 7. Migration and compatibility review

None. No schema, storage, protocol, configuration, or public API change. The
pane is hidden by `Default`, so a user who never presses `Ctrl-T` sees exactly
the M006-A behavior. MSRV 1.89 is preserved.

## 8. Security review

- The root is the explicit execution context, never `current_dir()`.
- Symlinks are never followed at any depth, including directory symlinks, so a
  workspace cannot point the tree outside itself. This is proven by two unit
  tests and one integration test, not by inspection.
- A tree-derived path can never contain `..` because it is built from validated
  component names, and it is additionally subject to `normalize_relative_path`
  on the way to `open_editor`.
- Entry names are length- and NUL-bounded; an over-long name is skipped rather
  than truncated, so a shown name is always a real on-disk name.
- The walk is bounded on node count and depth, so a hostile or pathological
  tree costs a bounded walk and a bounded allocation — a denial-of-service
  consideration, not just a performance one.
- The tree exposes no content, adding no secret-bearing surface.
- The tree is strictly read-only: no create, rename, delete, move, or copy
  exists, and every mutation remains on the existing agent/tool path.

## 9. Documentation and operations

- `architecture/tui.md` — a "Project-scoped file tree (M006-D)" section
  covering the navigator-not-content-path invariant, the bounds, symlink
  policy, authority source, focus/layout contract, and why the M006-A guard
  stays green with an unchanged scan list.
- `architecture/document.md` — records that the tree does not relax the
  dirty-buffer rejection and that a tree-derived path gets identical
  containment, `file.read` authorization, symlink policy, and dirty-buffer
  handling because it is literally the same code path.
- `plans/registry.md` and the subsystem roadmap: M006-D registered and
  reconciled.
- No new static guard. The plan preferred none because the read-only invariant
  is enforced by construction — the module has no content-reading API — and a
  regex rule over a module that cannot express a violation would be theatre.
- Operator note: `Ctrl-T` toggles the pane; `j`/`k` move, `h`/`l` collapse or
  expand, `Enter` opens a file or expands a directory, `r` re-walks, `Esc`
  leaves the tree.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | The `MAX_TREE_NAME_BYTES` cap of 255 equals the common `NAME_MAX`, so on macOS and Linux it is defensive rather than reachable — the filesystem rejects an over-long name first. | The guard is untestable as an active filter on those filesystems. | Accepted. The unconditional invariant (a shown name is never a truncated prefix) is asserted by `names_are_never_mangled`. A narrower cap would become actively testable if that is ever wanted. |
| low | The tree has no incremental refresh, search, filter, or git status. | A user editing files outside the TUI presses `r` to re-walk. | Deliberate scope exclusion in the plan (§5). Not a defect. |
| low | `Desktop E2E` did not run for PR `#94`; only `CI / verify` triggered. | No built-app trajectory evidence for the tree. | The tree is hidden by default and adds no visible change to the existing desktop E2E phases, so no E2E phase was extended. Worth revisiting if a graphical explorer is ever built. |

None is a regression introduced by this milestone, and none blocks closure.

## 11. Roadmap disposition

M006-D is closed. M006-A remains closed, so the M006 line now has two of five
sub-milestones delivered.

This reopens no closed scope: the causal-frontier and project-catalog
corrective records, and the M006-A closure, are untouched. The dirty-buffer
rejection in `src/lsp/mutation.rs` is unchanged and remains correct as written;
M006-D does not reach it because it never constructs a mutation.

Remaining M006 sequence: **M006-E** is unblocked and ready to scope, with an ADR
required only if apply-into-dirty-buffer is chosen; **M006-B** is gated on its
own ADR for the LSP read surface and its delivery/authorization path; and
**M006-C** depends on M006-B.

## 12. Registry updates

- `plans/registry.md` roadmap/status table: M006-D closed, implementation
  `de29e49e`, no ADR required.
- `plans/registry.md` implementation-plan table: M006-D closed, pointing at the
  plan and this closure.
- `plans/registry.md` closure table: M006-D closed with the hosted run id.
- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md`: M006-D moved
  from planned to implemented and closed; M006-A reconciled to closed.
- Nothing else is unblocked by this milestone.
