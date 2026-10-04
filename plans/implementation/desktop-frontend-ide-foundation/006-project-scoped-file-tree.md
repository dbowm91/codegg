# Desktop Frontend and IDE Foundation Milestone 006-D — Project-Scoped File Tree

Status: ready for handoff

Repository baseline: `94f38421` (M006-A merged)

Source roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#milestone-006--tui-ide-vertical-slice`
- `plans/subsystems/desktop-frontend-ide-foundation-m006-tui-presentation-audit.md` §14

Long-term requirements:

- `architecture/tui.md` — project-scoped authority, non-modal primary views
- `architecture/document.md` — the single text buffer stays the controller's

Applicable ADRs:

- `plans/adrs/ADR-0011-*` (accepted; M005 document foundation) — inherited.
- None required for this milestone. See §2.

Primary class: capability

## 1. Objective

Give the TUI a bounded, project-scoped file tree that navigates to documents by
opening them through the existing controller-backed editor path, so a user can
browse a workspace and open a file without typing `/open <path>` by hand.

## 2. Why this milestone is ready

- M006-A is merged and closed (`94f38421`), so `Route::Editor`,
  `TuiDocumentSession`, `open_editor`, and `EditorState` are all landed and the
  open path is proven.
- The roadmap assigns M006-D the dependency "M006-A for open/activate" and
  states no ADR. The audit lists ADRs only for M006-B (LSP read surface
  delivery/authorization) and conditionally for M006-E (apply into a dirty
  buffer). This milestone invents neither decision.
- The authority question is already settled by existing precedent, not by a
  new choice: the tree root is the active tab's explicit
  `ProjectExecutionContext::workspace_root`, exactly as `open_editor` already
  resolves it. No ambient `cwd` and no path-as-identity is introduced, so
  `check_tui_project_authority.py` is satisfied by construction.
- The milestone is one coherent vertical slice with one capability boundary:
  discover → render → open. It fits `plans/003-planning-process.md` §5.

## 3. Current implementation evidence

Landed and available for reuse:

- `src/tui/route.rs` — `Route::Editor`, documented as non-modal with the
  composer staying editable.
- `src/tui/app/render.rs:545` — `render_editor_view`, which currently occupies
  the whole viewport area.
- `src/tui/commands/editor.rs:115` — `open_editor(app, raw_path)`: lexical
  validation via `normalize_relative_path`, already-open short-circuit,
  `project_execution_context()` authority, scoped task spawn with request id
  and generation, and route navigation to `Route::Editor`.
- `src/tui/commands/editor.rs:62` — `normalize_relative_path`: rejects empty,
  over-long, NUL-bearing, absolute, drive-qualified, and `..`-escaping input.
- `src/tui/app/state/editor.rs:250` — `EditorState`, the existing per-view
  state holder with request/generation discipline.
- `src/tui/app/state/execution_context.rs:19` — `ProjectExecutionContext`
  carrying `workspace_root`, `project_id`, `workspace_id`, `session_id`.
- `src/tui/app/mod.rs:11491` — `index_files_recursive`, the composer's `@`
  file-mention indexer. It is the existing precedent for a bounded walk:
  depth-capped, skips symlinks, and skips a fixed ignore list
  (`node_modules`, `.git`, `target`, …).

Known gaps:

- No file-listing protocol variant exists. `grep` over
  `crates/codegg-protocol/src` finds no directory-listing request, so there is
  no daemon surface to reuse and none is needed.
- No tree, explorer, or file-pane component exists in `src/tui/components/`
  (only `sidebar.rs`, which is a chat/session sidebar, not a file navigator).
- The composer's `@` indexer is flat, non-interactive, and unpersisted; it
  cannot be reused as a tree without becoming a second, divergent
  implementation.

## 4. Invariants that must not regress

- The editor still obtains **all document text** through the M005 controller.
  The tree reads directory *entries* only and never file contents, and it
  opens files exclusively via `open_editor`.
- `scripts/check_tui_editor_text_authority.py` stays green and non-vacuous.
  Its `SCANNED_FILES` list is fixed at the five editor modules, so a new
  explorer module is outside its scan scope by construction; the plan must not
  widen that list.
- The tree root is `ProjectExecutionContext::workspace_root` resolved from the
  active project tab. No `std::env::current_dir()`, no path-as-identity, and
  no ambient authority. `scripts/check_tui_project_authority.py` stays green.
- Symlinks are never followed, at any depth, including directory symlinks.
- The walk is bounded on every axis: node count, depth, and per-entry name
  length. A pathological tree degrades to a truncation notice, never to a hang
  or an unbounded allocation.
- `Route::Editor` remains non-modal: the composer stays editable and the
  ordinary Session/Task composer behavior is unchanged.
- `/open <path>` keeps working exactly as in M006-A, including its validation
  and already-open behavior.
- No protocol, storage, migration, or authorization change.

## 5. Scope

### In scope

- A bounded, non-symlink-following workspace tree model.
- Persisted-per-view tree state: root, expansion set, selection, truncation
  notice, and whether the pane is visible.
- Rendering the tree as a left-hand pane inside `Route::Editor`.
- Motion and expand/collapse/open actions in the editor's existing key path.
- Opening a selected file through the existing `open_editor` path.
- Focused tests over the walk, the bounds, the state machine, and the
  open-into-editor trajectory.

### Explicitly out of scope

- **M006-B / M006-C (LSP).** No diagnostics, hover, definition, references,
  symbols, or semantic tokens. No protocol variant.
- **M006-E (agent edit/apply/review).** The tree never mutates a document and
  never proposes an apply.
- File creation, rename, delete, move, or copy. The tree is read-only
  navigation; every mutation stays on the existing agent/tool path.
- Git status, file icons beyond a directory/file distinction, search, filter
  text, and file watching. No incremental refresh: the tree is rebuilt on
  explicit user action.
- Reusing or generalizing the composer's `@` indexer into a shared component.
  The two have different bounds and lifetimes; merging them is a separate
  decision and would widen this milestone.
- A graphical (Monaco/Tauri) explorer. M006 is TUI-first by decision.
- Any change to `SCANNED_FILES` in the text-authority guard.

## 6. Required production changes

Describe behavior and ownership; the implementation agent may adjust
file-level mechanics to match current code.

### Core/domain

None. No domain type is added to `codegg-core`; the tree is a presentation
model over a directory listing, consistent with M006-A's presentation-model
approach in `src/tui/editor.rs`.

### Storage and migrations

None. Tree state is view state held on `App`, not persisted across restarts,
matching `EditorState`.

### Protocol and DTOs

None, deliberately. The roadmap scopes M006-B as the protocol slice; adding a
listing variant here would silently take M006-B's ADR-gated decision.

### Runtime and concurrency

- The walk runs on a scoped TUI task using the existing
  `spawn_scoped_registered_tui_task` + `TuiTaskKind` + request-id/generation
  discipline, with a new task kind for the tree. Blocking directory I/O must
  not run on the render path.
- A completion for a superseded request is discarded by generation, matching
  the stale-completion rule in `src/tui/async_cmd.rs`.
- No lock is held across the walk.

### Frontend or operator surface

- A new tree model module under `src/tui/` holding the node type, the bounded
  walk, and the expansion/selection state machine.
- Tree state on `App` alongside `EditorState`, with `Default` so the rest of
  the TUI is unaffected.
- A tree component under `src/tui/components/`, rendering indent guides, a
  directory/file distinction, the selected row, and a truncation notice.
- A layout branch in `render_editor_view` that splits the viewport when the
  pane is visible, and renders the full-width editor when it is not.
- Motion and toggle actions wired into the editor's existing key path, and
  their bindings added to the help overlay.
- The tree's own focus flag so the non-modal composer contract is explicit:
  tree-focused input does not reach the composer.

### Security and authorization

- The root is the explicit execution context, never `current_dir()`.
- Symlinks are skipped before canonicalization, matching the `is_symlink`
  check in `index_files_recursive`; following them would allow a workspace to
  point the tree outside itself.
- `..` never appears in a tree-derived path: the walk builds paths from
  validated names, and the open path still goes through
  `normalize_relative_path`, which rejects escapes.
- Entry names are length-bounded and NUL-checked before use.
- The tree exposes no content, so it adds no secret-bearing surface.

### Documentation and static guards

- `architecture/tui.md`: the tree pane, its authority source, its bounds, and
  the read-only invariant.
- `architecture/document.md`: note that the tree is a navigator, not a second
  buffer.
- `plans/registry.md` and the roadmap: register M006-D, and record that
  M006-D needed no ADR.
- A new static guard is warranted if, and only if, the read-only invariant is
  not already enforced by the existing editor text-authority guard. The plan's
  preference is **no new guard**: the invariant is enforced by construction
  (the module has no content-reading API and opens only through
  `open_editor`), and a new regex guard over a module that cannot express a
  violation would be theatre. If the implementation does introduce any direct
  content read, that is a stop condition (§14), not a guard to write.

## 7. Ordered work packages

### Work package A — Bounded tree model and walk

Intent: one pure, testable module that turns a workspace root into a bounded
tree.

Required changes: a `TreeNode` (name, workspace-relative path, depth, kind,
children) and a walk that enforces a maximum node count, a maximum depth, and
a per-entry name bound; skips symlinks; applies an ignore list; sorts
directories before files then lexically within each group; and records whether
it truncated, so the UI can say so rather than silently showing a partial
tree. Expansion and selection are pure state transitions on a selected index
or path.

Acceptance evidence: unit tests for ordering, symlink refusal at file and
directory level, the depth cap, the node cap producing a truncation notice,
and the name bound. A `..`-containing name cannot be produced.

### Work package B — Tree state on `App`

Intent: hold tree state with the same discipline the editor already uses.

Required changes: a state struct with root, visible flag, expansion set,
selected path, cached nodes, truncation notice, and request/generation
tracking; a `Default`; and a `TuiTaskKind` variant for the walk.

Acceptance evidence: `Default` leaves the pane hidden and empty so every
existing TUI test is unaffected; a stale completion is discarded by generation.

### Work package C — Tree component and layout branch

Intent: render the tree without disturbing the editor.

Required changes: a component rendering indent guides, a directory/file
distinction, the selected row, and the truncation notice; a horizontal split
in `render_editor_view` when visible, with a minimum pane width that yields
the full-width editor rather than a degenerate one on a narrow terminal.

Acceptance evidence: the editor renders unchanged when the pane is hidden;
`render_editor_view` still borrows the controller snapshot and drops it within
the call, so the frame holds no document buffer.

### Work package D — Motion, expand/collapse, and open

Intent: make the tree usable and connect it to the existing open path.

Required changes: motion within the visible tree, expand/collapse on a
directory, and open on a file by calling `open_editor` with the selected
workspace-relative path; a tree focus flag so these keys do not reach the
composer; help-overlay bindings; and an explicit action to rebuild the tree.

Acceptance evidence: opening a file from the tree produces the same controller
attachment and route behavior as `/open`, and an already-open document still
short-circuits.

### Work package E — Trajectory and guard verification

Intent: prove the vertical slice end to end and prove no boundary regressed.

Required changes: a trajectory test over a scripted transport that opens a
file from the tree and asserts the controller, mirroring M006-A's trajectory
style; plus running the existing guards.

Acceptance evidence: the trajectory passes; `check_tui_project_authority.py`,
`check_tui_editor_text_authority.py`, and `check_execution_ownership.py` are
green; `scripts/verify.sh quick` exits 0.

## 8. Failure, cancellation, restart, and contention semantics

- **Unreadable directory:** the walk skips that entry and continues; it does
  not abort. A permission-denied subdirectory must not blank the whole tree.
- **Root unreadable or absent:** the pane shows an explicit error state and the
  editor remains fully usable. The tree never becomes a precondition for the
  editor.
- **Truncation:** hitting a bound is a first-class, displayed outcome, not a
  silent partial view.
- **Stale completion:** a walk that finishes after the user switched project
  tab or rebuilt the tree is discarded by generation, so a tree is never
  rendered against the wrong workspace root.
- **Project switch:** the tree's root, expansion, and selection reset, because
  a workspace-relative path from the previous project is meaningless in the
  next.
- **Duplicate delivery:** repeated rebuild requests for the same generation
  collapse to the last one.
- **Cancellation:** rebuilding replaces the in-flight walk; the older result is
  dropped by generation rather than blocking the new one.

## 9. Compatibility and migration

No schema, storage, protocol, or configuration change. `Route::Editor` keeps
its existing identity, and its empty-state and open-document behavior are
unchanged when the pane is hidden — which is the default, so every existing
M006-A test and every existing TUI test continues to hold.

No version skew is introduced because no wire surface changes. This is the
concrete reason M006-D is separable from M006-B: the LSP read surface will
need a protocol-migration and version-skew statement, and the tree does not.

## 10. Required tests

### Focused unit tests

- Walk: ordering (directories before files, then lexical), symlink refusal for
  both file and directory symlinks, depth cap, node cap with truncation,
  per-entry name bound, unreadable directory skipped rather than fatal.
- Path safety: a tree-derived path is always workspace-relative and never
  contains `..`; it is accepted by `normalize_relative_path`.
- State machine: expand/collapse is idempotent, selection survives expansion,
  and rebuild resets per project.

### Integration tests

- A trajectory test over the scripted transport: rebuild the tree, expand a
  directory, open a file, and assert the controller attachment and route.
- The full `tui::` lib suite, which must remain green because the pane is
  hidden by default.

### Restart and recovery tests

Not applicable; no durable state. A fresh `App` must start with the pane
hidden, which is the cold-start case.

### Contention and cancellation tests

- Rebuild during an in-flight walk: the stale result is discarded and the tree
  reflects only the latest request.
- Project switch during an in-flight walk: the older root's result is
  discarded.

### Security and negative tests

- A workspace containing a symlink pointing outside the root proves the tree
  does not follow it.
- A symlink loop proves termination.
- A path-shaped entry name (for example containing `..`) is either skipped or
  provably cannot produce an escaping path.
- A tree with more nodes than the cap proves the truncation notice appears
  rather than the walk running unbounded.

### Migration and compatibility tests

Not applicable; no migration.

## 11. Required verification commands

```bash
# narrow tests first
cargo test --lib tui::explorer
cargo test --lib tui::

# static guards (change-triggered)
python3 scripts/check_tui_project_authority.py
python3 scripts/check_tui_editor_text_authority.py
python3 scripts/check_execution_ownership.py

# formatting and linting
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings

# broader suite appropriate to the change
scripts/verify.sh quick
cargo nextest run --workspace --locked --profile ci
```

Hosted `CI / verify` and `Desktop E2E` on the PR head supply the gating
evidence. Do not claim commands that were not actually run in the closure
record.

## 12. Documentation updates

- `architecture/tui.md`: the tree pane under `Route::Editor`, its authority
  source, its bounds, its read-only invariant, and the note that it is outside
  the editor text-authority guard's scan scope by construction.
- `architecture/document.md`: the tree is a navigator; the controller remains
  the only owner of document text.
- `plans/registry.md`: register M006-D and its closure.
- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md`: move M006-D
  from "unblocked; ready for planning" to implemented, and record that it
  needed no ADR.
- No `architecture/overview.md` change beyond a pointer if the new module is
  significant enough to list.

## 13. Acceptance criteria

- The TUI shows a bounded, project-scoped file tree inside `Route::Editor`,
  hidden by default, and every pre-existing TUI test still passes unchanged.
- The tree root is the active tab's explicit `workspace_root`; no ambient cwd
  or path-as-identity is introduced.
- Symlinks are never followed, and a symlink loop terminates.
- Depth, node count, and entry-name length are all bounded, and truncation is
  displayed rather than silent.
- Opening a file from the tree produces the same controller attachment, route,
  and already-open short-circuit as `/open`.
- The editor still obtains all document text through the controller:
  `check_tui_editor_text_authority.py` is green and its `SCANNED_FILES` list
  is unchanged.
- `check_tui_project_authority.py` and `check_execution_ownership.py` are
  green; `scripts/verify.sh quick` exits 0.
- The `Route::Editor` composer remains editable and the tree focus flag is
  explicit.
- Hosted `CI / verify` is green on the PR head with the full workspace sweep
  completing.

## 14. Stop conditions

Stop and report rather than improvise when:

- the tree appears to need a new protocol variant or daemon handler — that is
  M006-B and its ADR, not this milestone;
- the tree would need to read file *contents* — the editor's text authority
  forbids it and this milestone must not widen that guard;
- the walk cannot be made bounded on every axis, or a symlink loop cannot be
  shown to terminate;
- a tree-derived path cannot be proven workspace-relative and `..`-free;
- compositing the `@` indexer and the tree appears to be required — that is a
  separate decision;
- the work would expand into M006-E's mutation surface or M006-C's LSP
  presentation;
- an unresolved architecture decision materially changes ownership.

## 15. Closure evidence required

- The walk's bound values and the measured behavior at each bound, including
  the truncation notice.
- Symlink, symlink-loop, and `..`-entry negative-test results.
- The tree→`open_editor` trajectory result and evidence it matches `/open`.
- Proof the editor's text authority is intact: the guard green and
  `SCANNED_FILES` unchanged (a diff of the guard file showing no change to
  that list).
- The exact guard, fmt, clippy, `verify.sh quick`, and suite commands run,
  with results and counts.
- The hosted `CI / verify` and `Desktop E2E` run ids, with an explicit
  statement of whether the full workspace sweep completed.
- A statement that no protocol, storage, migration, or authorization change
  was made, and that no historical closure record was edited.

## 16. Handoff notes

- The tree is a **navigator**, not a second buffer and not a second content
  path. If an implementation starts reading file bytes, stop.
- `open_editor` already does lexical validation, authority resolution, scoped
  spawn, and route navigation. Reuse it; do not reimplement an open path.
- The composer's `index_files_recursive` is the bound/symlink precedent to
  follow, not a component to import.
- Keep the pane hidden by default so the M006-A test surface is untouched.
- The layout split must degrade to the full-width editor on a narrow terminal
  rather than producing a zero-width pane.
- `Default` on the new state is what keeps the rest of the TUI compiling and
  passing unchanged; get it right first.
- No ADR is needed. If the implementation finds itself making an ownership or
  delivery decision, that is the stop condition, not something to decide
  in-pass.
