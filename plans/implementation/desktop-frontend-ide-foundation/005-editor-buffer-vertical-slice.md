# Desktop Frontend and IDE Foundation Milestone M006-A — Editor Buffer Vertical Slice

Status: ready for handoff

Repository baseline: `614e983e5a4f57718ac65d1ecee021719500fd75`

Source roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#milestone-006--tui-ide-vertical-slice`
- M006 decomposition table: `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#m006-decomposition`

Planning basis:

- `plans/subsystems/desktop-frontend-ide-foundation-m006-tui-presentation-audit.md` — the
  presentation audit that discharged the M006 gate and produced this decomposition. This
  plan implements its §14 M006-A row and must not silently absorb M006-B…M006-E scope.

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior` — the TUI remains the
  reference frontend and receives first-class development priority.
- `plans/000-long-term-specification.md` non-goals — a CRDT collaborative source editor is
  explicitly out of product scope, so M006-A MUST NOT introduce one.
- `plans/001-terminology-and-domain-model.md` — normative identity and language.

Applicable ADRs and architecture:

- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md` (accepted) —
  controlling. Frontends own presentation state; undo/redo is frontend-owned; syntax
  highlighting is deferred to this milestone; CRDT/OT is deferred.
- `plans/adrs/ADR-0010-desktop-frontend-and-shared-client-boundary.md` (accepted).
- `architecture/document.md` — the `document.v1` contract and M005 limitations.
- `architecture/tui.md` — TUI ownership, `Route`, focus, and the sidebar's projection role.
- `architecture/client.md`, `architecture/authorization.md`.

Predecessor closure (hard dependency, satisfied):

- `plans/closure/editor-document-foundation-post-closure-corrective/001-status.md` — current
  strict authority for M005-D and parent M005; implementation `1dcbce6d`; hosted root CI
  `37177976742` and Desktop E2E `37177976771` green.

Primary class: capability

> The editor buffer is user-visible behavior. It may not be described as an IDE, and the
> daemon document service, LSP, and agent tooling are **not** part of this milestone.

## 1. Objective

Deliver the first user-visible TUI presentation of the shared M005 document foundation:

- a non-modal editor primary view that renders a workspace-relative document **exclusively**
  from `codegg_client::DocumentController`'s optimistic replica;
- a line-number gutter, current-line highlight, horizontal scroll, and a deterministic
  viewport;
- cursor and anchor/selection over UTF-8 byte offsets, clamped to byte boundaries;
- vi-style motion and editing commands, with frontend-owned undo/redo;
- local edits applied through `apply_local`, with checked save, conflict, reload, and close
  mapped to existing controller states;
- a bounded, explicit open path for one workspace-relative path.

One bounded outcome: **a user can open a document, edit it, see their edits, undo, and save
it, and the TUI holds no second text buffer.**

## 2. Why this milestone is ready

Closed hard dependencies:

- M005-A/B/C and M005-D are strictly closed under
  `plans/closure/editor-document-foundation-post-closure-corrective/001-status.md`.
- The `document.v1` native surface is complete and additive: `DocumentCapabilities`,
  `DocumentOpen`, `DocumentSnapshotGet`, `DocumentStatusGet`, `DocumentWriterAcquire`,
  `DocumentChange`, `DocumentSave`, `DocumentReload`, `DocumentClose`
  (`crates/codegg-protocol/src/core.rs:1796-1834`).
- `DocumentController` exposes the full lifecycle M006-A needs — `open`, `apply_local`,
  `flush`, `save`, `reload_from_disk`, `resync`, `reconnect`, `poll_status`, `close`
  (`crates/codegg-client/src/document.rs:210-1081`).

Stable interface dependencies:

- `DocumentSnapshot` already provides `len_lines`, `line_range`, `read_bytes`,
  `byte_to_position`, `position_to_byte`, and `is_byte_boundary`
  (`crates/codegg-document/src/buffer.rs:33-89`). M006-A needs **no** new text crate and
  **no** new line-index implementation.
- `Route::Workspace` is the accepted precedent for a non-modal primary view with its own
  focus concept (`src/tui/route.rs:2-12`; `architecture/tui.md:652`). M006-A follows that
  shape rather than inventing a second navigation model.
- `TuiDocumentSession` is the correct ownership seam and needs only a consumer
  (`src/tui/document_session.rs:32`).

No ADR is required for M006-A. It introduces no protocol, no storage, no LSP, no
authorization surface, and no agent semantics. The two ADR-gated decisions identified by the
audit belong to M006-B and M006-E and are explicitly out of scope.

## 3. Current implementation evidence

At the baseline:

- **The seam exists and has no consumer.** `TuiDocumentSession` is declared at
  `src/tui/mod.rs:1481` and is never constructed in production code.
  `TuiDocumentPresentation` has no reference outside its own file; its `cursor_byte`,
  `selection`, and `viewport_line` fields are exercised only by the in-file unit test.
- **There is no editor widget.** `src/tui/components/` has no `editor.rs` or equivalent.
- **The layout cannot host one.** `TuiLayout::split` yields `[main, sidebar]`;
  `session_layout` yields `[header, viewport, prompt, footer]`
  (`src/tui/layout.rs:74` and `:91`). `Route` has three arms
  (`src/tui/route.rs:2-12`, dispatched at `src/tui/app/render.rs:511-517`). There is no pane,
  split, or focusable-region concept beyond sidebar and prompt.
- **No editing primitives exist.** `PromptWidget` is one `String` with a byte `cursor` and a
  `horizontal_scroll` offset (`src/tui/components/prompt.rs:10-26`). There is no selection
  anchor, word motion, kill-line, yank, or undo/redo anywhere in the TUI. `InputMode` is only
  `Insert | Normal` and that "normal" mode is a command mode, not a vi-style buffer mode.
- **Widget support is thin.** `ratatui 0.30` with `default-features = false`
  (`Cargo.toml:143`) provides no gutter, current-line highlight, or per-line style
  primitive. `Wrap` is available and used elsewhere. Line numbers exist only in the diff
  dialog and the read-only `SourcePreviewDialog` (capped at 500 lines / 1 MiB,
  `src/tui/components/dialogs/source_preview.rs:15-16`).
- **A reusable diff path exists for later milestones.** `DiffViewer` over `similar` with
  `DiffMode::{Inline, SideBySide}` (`src/tui/components/diff.rs:9, 15-18`). Not used by
  M006-A.
- **Execution-ownership precedent is wrong to copy.** `JobScheduler` and
  `JobSubmissionService` have **zero** references under `src/tui`; the TUI spawns via
  `TuiTaskRegistry`/`spawn_blocking` (for example `src/tui/file_diff.rs:35-77`). M006-A's
  document I/O goes through the controller and the `document.v1` protocol, so it must not
  introduce filesystem reads behind that precedent.
- **Slow-handler discipline applies.** `apply_local` is synchronous, but `open`, `save`,
  `reload_from_disk`, `resync`, `reconnect`, and `poll_status` are async. M006-A's async
  calls MUST use `spawn_tui_task` with `finish(request_id)` / `fail(request_id, err)` and a
  stale-completion test (`src/tui/async_cmd.rs`).

## 4. Invariants that must not regress

1. The daemon `DocumentService` remains the sole owner of canonical open-document text.
2. The TUI holds **no second text buffer**. The editor renders from
   `DocumentController::snapshot()` and every mutation goes through `apply_local`.
3. Presentation state — cursor, selection, viewport, undo history — is frontend-owned and
   never transmitted as authority.
4. Disk remains the durable source of truth; save is checked against the recorded disk base
   and a mismatch becomes a conflict rather than an overwrite.
5. External mutation never silently overwrites a dirty buffer.
6. One writer lease per document per transport connection; a second open of the same
   document is rejected (`AlreadyOpen`).
7. The client remains an optimistic replica. Undo/redo MUST NOT be implemented as a
   daemon-side or protocol-side operation, and MUST NOT bypass the controller queue.
8. Project/workspace identity is resolved from the active tab's explicit execution context.
   No ambient `project_dir`-style authority may be introduced —
   `scripts/check_tui_project_authority.py` enforces this.
9. Slow TUI handlers remain guarded by `spawn_tui_task` completion tokens.
10. Root Rust 1.89, default-feature CLI builds, and the existing TUI behavior remain
    unaffected.

## 5. Scope

### In scope

- `TuiDocumentSession` construction from the active project's execution context, plus
  lifecycle wiring for open, save, reload, resync, reconnect, and close.
- An editor presentation model over `TuiDocumentPresentation`: cursor, anchor/selection,
  viewport, and a bounded frontend undo/redo stack.
- An editor widget: line-number gutter, current-line highlight, horizontal scroll, and
  deterministic viewport windowing over a bounded visible slice.
- A non-modal editor primary view and the layout/focus branch required to host it.
- vi-style motion and editing commands sufficient for open, navigate, edit, undo, redo,
  save, and close.
- A bounded explicit open path: a TUI command taking one workspace-relative path, validated
  through the existing `DocumentOpen` authorization and path-containment rules.
- Visible controller state: synced, local-dirty, flushing, conflict, disconnected,
  resync-required, read-only, and gone-with-draft.
- Focused tests, one new static guard, and documentation updates.

### Explicitly out of scope

- **All LSP work** — no diagnostics, semantic tokens, hover, definition, references,
  symbols, or completion. The audit established that no LSP read operation exists in the
  native protocol; adding one is M006-B and requires an ADR.
- **File tree / explorer** — M006-D. M006-A opens a path by explicit command only.
- **Agent edit/apply/review** — M006-E. M006-A MUST NOT change LSP preview-apply behavior;
  the dirty-buffer rejection (`lock_clean_paths` at `src/lsp/mutation.rs:197` and
  `is_managed_document_dirty` at `src/lsp/mutation.rs:240-245`) stays exactly as it is.
- **Soft wrap** — deferred. M006-A ships hard wrap with horizontal scroll. See §6.
- **Monaco, Tauri, or any graphical editor** — long-term, per the roadmap and ADR-0011 §10.
- **CRDT/OT, collaborative editing, or durable unsaved recovery** — explicit product
  non-goals and ADR-0011 deferrals.
- **Multi-document tabs, split panes, or a workspace browser.**
- **Terminal composition, Git/worktree/job views, or agent-tree changes.**
- **Any protocol, storage, migration, or authorization change.**

## 6. Required production changes

### Core/domain

No domain change. The daemon `DocumentService` and `codegg-document` are consumed
unchanged.

### Storage and migrations

None. No migration, no schema version change, no new persisted state. Undo history is
process-local and is never persisted.

### Protocol and DTOs

None additive. M006-A uses only the existing `document.v1` requests. If implementation
reveals that an existing request cannot express a required editor behavior, **stop** and
report per §14 rather than extending the protocol.

### Runtime and concurrency

- Construct `TuiDocumentSession` from the active tab's explicit project/workspace execution
  context. The editor holds at most one attachment; switching documents closes the previous
  controller before opening the next (one controller owns one attachment).
- All async controller calls use `spawn_tui_task` with `finish(request_id)` /
  `fail(request_id, err)`. Include the stale-completion test required by
  `src/tui/async_cmd.rs`.
- **Undo/redo is frontend-owned and local.** Record an undo entry as the inverse
  `TextTransaction` applied through `apply_local`, never as a protocol operation. Cap undo
  depth and total retained bytes with named constants.
- **Undo/redo must not survive reconciliation.** Clear the undo stack on
  `resync`, `reconnect`, `reload_from_disk`, `close`, and any open of a different document.
  A divergent draft is retained by the controller in a recovery state; the editor surfaces
  that state and MUST NOT replay or discard the draft on the user's behalf.
- Map every `DocumentControllerError` to explicit user-visible presentation. `LifecycleBusy`
  and `QueueFull`/`QueueBytes` are expected conditions, not crashes.
- **Soft wrap is deliberately excluded.** Hard wrap with horizontal scroll keeps line↔screen
  mapping one-to-one, which is what makes cursor math, the gutter, and current-line
  highlight deterministic. Soft wrap changes the line→row mapping and is a separate
  milestone. Record this decision and the deferred soft-wrap work in the closure record.

### Frontend or operator surface

- An editor presentation struct that owns cursor, anchor/selection, viewport, and undo depth
  and holds **no text**. Keep `TuiDocumentPresentation` as the ownership record for
  cursor/selection/viewport; extend it rather than introducing a parallel buffer.
- Clamp every cursor and selection offset with `is_byte_boundary` and
  `byte_to_position`/`position_to_byte`; never index a line by a raw user-facing offset.
- A new non-modal editor primary view following the `Route::Workspace` precedent: a `Route`
  arm plus an explicit focus concept so the editor does not steal composer input. The
  ordinary Session/Task composer remains editable while the editor is open unless the user
  explicitly focuses the editor buffer.
- Line-number gutter width adapts to the document's line count and must not jitter as the
  cursor moves.
- Render only a bounded visible slice. Do not materialize the whole document into a
  `Vec<Line>` per frame.
- A bounded open command for one workspace-relative path, reusing existing
  `validate_path`/containment behavior rather than introducing new path resolution.

### Security and authorization

- The editor MUST NOT read files directly. All text arrives through
  `DocumentController::snapshot()`, which is daemon-authorized. Do not add `read_to_string`
  or `read_dir` behind the editor.
- `DocumentOpen` attaches read-only with `file.read`; writer acquisition is a separate
  `file.modify` operation. M006-A MUST NOT silently escalate to writer.
- Project/workspace identity comes from the explicit execution context, never from a path
  or cwd. `scripts/check_tui_project_authority.py` must stay green.
- Render file contents only; never log buffer text. Undo history is in-memory and must not
  be emitted to logs, notifications, or the audit payload.

### Documentation and static guards

- One new static guard asserting the editor presentation layer holds no second text buffer
  and that no editor path reads the filesystem directly. `eggsact` is an external crate
  dependency, not a workspace member, so it is not a home for a repository guard: add a
  focused `scripts/check_*.py` guard following the pattern of
  `scripts/check_tui_project_authority.py`, and register it in `scripts/verify.sh quick` and
  the CI `verify` job. Do not leave the invariant as a prose promise.
- `check_tui_project_authority.py` and `check_execution_ownership.py` remain applicable and
  must pass unchanged.

## 7. Ordered work packages

### Work package A — Editor presentation model and no-second-buffer guard

Intent: establish the state owner and prove the invariant before any rendering exists.

Required changes:

- Extend `TuiDocumentPresentation` into the editor's presentation record: cursor byte,
  anchor/selection, viewport line/column, and a bounded undo stack of inverse transactions.
  No text.
- Boundary-safe offset helpers over `DocumentSnapshot`.
- The no-second-buffer guard from §6, wired into `verify.sh quick` and CI.

Acceptance evidence:

- A guard failure is demonstrated by a temporary violating construct and then reverted.
- Unit tests for offset clamping at UTF-8 boundaries, empty documents, and cursor past end.

### Work package B — Controller lifecycle wiring

Intent: connect the TUI to the already-closed M005 contract.

Required changes:

- Construct `TuiDocumentSession` from the active execution context; hold one attachment.
- Wire `open`, `save`, `reload_from_disk`, `resync`, `reconnect`, `close`, and `poll_status`
  through `spawn_tui_task` with completion tokens and a stale-completion test.
- Map every `DocumentState` and `DocumentControllerError` to explicit presentation.
- Clear undo history on every reconciliation transition.

Acceptance evidence:

- A TUI-kind trajectory test that opens, edits, saves, and reopens through the editor path,
  in the style of the existing `tui_kind_controller_keeps_edit_through_save_and_reopen`.
- A test that `LifecycleBusy`, `QueueFull`, and conflict each produce their own visible
  state rather than a generic error.

### Work package C — Editor primary view and layout branch

Intent: give the editor a home without inventing a second navigation model.

Required changes:

- A `Route` arm for the editor, following `Route::Workspace`'s documented shape.
- An explicit focus concept for the editor region that composes with, and does not
  unconditionally steal, composer input.
- The layout branch that hosts the editor alongside the existing bands.
- A bounded open command for one workspace-relative path.

Acceptance evidence:

- Opening and closing the editor leaves session composition, sidebar projections, and the
  prompt in their prior state.
- The composer is not corrupted by editor keystrokes when the editor is not focused, and
  vice versa.

### Work package D — Editor widget rendering

Intent: render text, gutter, and viewport deterministically.

Required changes:

- Line-number gutter sized from `len_lines`, stable across cursor movement.
- Current-line highlight.
- Horizontal scroll bound to the longest visible line, with the cursor kept in view.
- Bounded visible-slice windowing; no whole-document materialization per frame.
- Read-only presentation when the attachment is not the writer.

Acceptance evidence:

- Render tests over a bounded fixture asserting gutter stability, current-line movement, and
  scroll clamping at buffer edges.
- A large-file fixture proving per-frame work is bounded by the viewport, not the document.

### Work package E — Motion, editing, undo/redo, and save

Intent: complete the user loop.

Required changes:

- vi-style motion and editing commands over the presentation model only.
- Undo/redo as inverse local transactions through `apply_local`, with depth/byte caps.
- Save bound to the controller, with conflict surfaced explicitly and never auto-resolved.
- Help-overlay text for every new binding.

Acceptance evidence:

- A deterministic command-sequence test asserting resulting text, cursor, and selection.
- An undo/redo test proving the controller snapshot matches after each step.
- A conflict test proving the draft is retained and surfaced, not overwritten.
- A test proving undo history does not survive resync/reload/close.

## 8. Failure, cancellation, restart, and contention semantics

- **Partial failure on open:** the editor shows the daemon error and holds no attachment. It
  MUST NOT display a partially populated buffer.
- **Save transport failure:** the pending draft is retained by the controller and the state
  becomes `Disconnected`; the editor surfaces it and MUST NOT drop local text.
- **Disk conflict:** state becomes `Conflict`. The editor presents the conflict and the
  retained draft. M006-A MUST NOT auto-reload, auto-replay, or auto-discard.
- **Resync/reconnect:** edits freeze during reconciliation; on success undo history is
  cleared and scheduling is rearmed by the controller; on failure the previous usable state
  is restored. A divergent draft is retained in a recovery state.
- **Daemon restart:** unsaved local drafts are process-local and may be lost. M006-A MUST
  NOT write an orphaned draft over newly opened disk state, and MUST NOT claim durability.
- **Stale completions:** an async result for a closed or switched document MUST be discarded.
  This is the stale-completion test from `src/tui/async_cmd.rs`.
- **Concurrent callers:** one editor attachment at a time. A second open of the same document
  is rejected with `AlreadyOpen`; the editor surfaces it rather than orphaning the first
  attachment.
- **Repeated rapid edits:** bounded by the controller's configured transaction and byte
  limits; the editor surfaces `QueueFull`/`QueueBytes` and does not retry internally.
- **Undo during a destructive phase:** edits are rejected as `LifecycleBusy`; undo must fail
  the same way rather than queueing a transaction the controller will refuse.

## 9. Compatibility and migration

- No wire change, so no protocol negotiation or version-skew behavior is introduced.
  `PROTOCOL_VERSION` and `document.v1` are untouched.
- No storage change, so no migration and no `STORAGE_LAYOUT_VERSION` update.
- Existing TUI sessions, routes, and dialogs are unchanged when the editor is closed. This is
  a compatibility requirement, not a convenience: a user on the current build must see no
  behavioral difference until they open a document.
- No configuration keys are added. A future default (for example, restoring the last opened
  document) requires a separate decision and is out of scope.
- Nothing is removed. There is no legacy editor path to retire.

## 10. Required tests

### Focused unit tests

- Offset clamping: UTF-8 boundary, empty document, cursor past end, selection inversion.
- Line/byte conversion via `byte_to_position` / `position_to_byte`.
- Gutter width stability across cursor movement and across documents of differing length.
- Horizontal scroll clamping at both buffer edges.
- Undo/redo stack: push, cap, inverse-transaction correctness, and clear-on-reconcile.
- Every new presentation state renders distinctly, including conflict and
  gone-with-draft.

### Integration tests

- Open → edit → undo → redo → save → reopen through the editor path, asserting the
  controller snapshot at each step.
- Read-only attachment renders and refuses edits.
- Switching documents closes the previous controller before opening the next.
- The editor coexists with an active session: prompt composition and sidebar projections
  survive editor open/close.

### Restart and recovery tests

- Resync with a matching queue clears undo history and preserves text.
- Resync with a divergent draft retains the draft in a recovery state and clears undo.
- Reconnect failure restores the previous usable state.
- Daemon restart leaves the editor with no attachment and no fabricated draft.

### Contention and cancellation tests

- A slow async open whose completion arrives after the editor closed is discarded.
- A rapid 25-edit burst produces serial change submission and maximum change concurrency of
  one, mirroring the existing `blocked_flush_owns_one_worker_through_a_local_edit_burst`
  evidence.
- Save while a destructive phase is active is rejected as `LifecycleBusy`, and undo during
  that phase is rejected identically.

### Security and negative tests

- Path traversal and symlink components in the open command are rejected by existing
  containment rules; the editor displays no file content.
- A read-only attachment cannot be escalated to writer by any editor command.
- No editor code path reads a file directly; enforced by the new static guard.
- Buffer text does not appear in logs or notification payloads.

### Migration and compatibility tests

- None required — M006-A introduces no migration. Assert instead that the default-feature
  workspace build and the existing TUI render tests are unchanged.

## 11. Required verification commands

```bash
# narrow tests first
cargo test -p codegg-document --locked
cargo test -p codegg-client --locked
cargo test --lib tui::document_session --locked
cargo test --lib tui::editor --locked

# static guards
python3 scripts/check_tui_project_authority.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/<new-no-second-buffer-guard>.py

# formatting and linting
cargo fmt
cargo clippy --workspace --all-targets --locked -- -D warnings

# broader suite appropriate to the change
scripts/verify.sh quick
cargo test --test tui_render --locked
```

Do not claim commands that were not actually run in the closure record. If the TUI render
harness requires a feature flag or fixture that does not exist yet, create it as part of
Work package D and record that in the closure record rather than dropping the test.

## 12. Documentation updates

- `architecture/document.md` — record that the TUI seam now has a real consumer, and restate
  the M005 limitations that still hold (one writer per document, process-local drafts, no
  GUI editor, LSP preview apply still rejected on a dirty buffer).
- `architecture/tui.md` — document the editor route, the focus model, the presentation-state
  ownership, and the hard-wrap decision with soft wrap recorded as deferred. Update the
  state hierarchy to include editor presentation state.
- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md` — mark M006-A closed only
  after the closure record exists; leave M006-B…M006-E untouched.
- `plans/registry.md` — register the M006-A plan, then update to its closure on acceptance.
- `README.md` — only if the editor is genuinely usable enough to document; otherwise
  explicitly do not touch it.
- Do **not** edit `plans/000-long-term-specification.md`,
  `plans/001-terminology-and-domain-model.md`, `plans/002-long-term-roadmap.md`, or
  ADR-0011. A corrective implementation pass is not justification for changing long-term
  direction. The "IDE" naming ambiguity and the M006 label collision are recorded in the
  audit §12; correcting them is a separate, user-directed terminology decision.

## 13. Acceptance criteria

1. A user can open one workspace-relative document, see it rendered with a line-number
   gutter and current-line highlight, edit it, undo, redo, and save it — entirely within the
   TUI.
2. The editor renders only from `DocumentController::snapshot()`; the static guard fails on
   any second text buffer or direct filesystem read in the editor path.
3. Every `DocumentState` and `DocumentControllerError` has distinct, tested presentation.
   Conflict and gone-with-draft drafts are retained and surfaced, never auto-resolved.
4. Undo/redo is frontend-local, depth-capped, applied through `apply_local`, and cleared on
   every reconciliation transition.
5. All new async TUI calls are `spawn_tui_task`-guarded and have a stale-completion test.
6. `scripts/verify.sh quick` is green, including `check_tui_project_authority.py`,
   `check_execution_ownership.py`, and the new guard.
7. `cargo fmt` and workspace Clippy with `-D warnings` are clean.
8. No protocol, storage, migration, or authorization change landed.
9. Closing the editor restores the exact prior TUI state; no regression in existing TUI
   render tests.
10. LSP, file tree, agent edit/apply, soft wrap, and graphical editor work are provably
    absent from the diff.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- implementing the editor appears to require a new `document.v1` request or DTO — the
  protocol is frozen for this milestone;
- the editor appears to need filesystem access outside the controller — that would break the
  daemon-text-authority invariant;
- undo/redo cannot be made frontend-local without weakening controller queue or
  reconciliation behavior;
- the layout cannot host the editor without changing session-composition behavior for
  existing users;
- soft wrap appears necessary to deliver a coherent viewport — it is deferred, and its
  absence is a recorded decision, not a blocker;
- any change would pull in LSP, a file tree, agent apply semantics, or CRDT (M006-B,
  M006-D, M006-E, or a product non-goal respectively);
- project/workspace identity cannot be resolved from the explicit execution context without
  ambient path authority;
- a static guard or the canonical invariant set cannot be satisfied as written;
- the milestone would expand into another subsystem roadmap.

## 15. Closure evidence required

The closure record at `plans/closure/desktop-frontend-ide-foundation/005-m006-a-status.md`
must contain:

- implementation commits, and confirmation that no protocol, storage, or migration change
  landed;
- a requirement-to-evidence matrix mapping each §13 acceptance criterion to a named test,
  guard, or command outcome;
- the full command list from §11 with actual outcomes, including any command that failed and
  its classification;
- evidence that the no-second-buffer guard was demonstrated to fail on a violating construct
  before being reverted;
- the integration trajectory results, including the 25-edit burst concurrency evidence and
  the stale-completion test;
- restart/recovery results for resync, reconnect, and daemon restart;
- security evidence: containment rejection, read-only enforcement, and confirmation that no
  buffer text reached logs;
- the recorded hard-wrap decision and the deferred soft-wrap work;
- documentation updates actually made, and an explicit statement if `README.md` was
  deliberately not changed;
- unresolved findings classified by severity, with any medium or high finding keeping M006-A
  open;
- a recommendation of closed, conditionally closed, corrective pass required, or blocked.

Per `plans/003-planning-process.md` §2.5, a commit message asserting closure is not closure
evidence. A build that compiles is not closure.

## 16. Handoff notes

- **Authority order:** ADR-0011 and the M005 closure record outrank this plan. Where
  repository evidence conflicts with this plan, preserve the canonical invariant, record the
  discrepancy, and make the smallest coherent adjustment.
- **Do not edit** `src/tui/document_session.rs`'s ownership shape, the daemon
  `DocumentService`, `codegg-document`, `codegg-client`'s controller semantics, or any
  closure record. If the seam looks wrong, report it — M005 is strictly closed.
- **The audit is the scoping contract.** `plans/subsystems/desktop-frontend-ide-foundation-m006-tui-presentation-audit.md`
  §14 defines the M006-A…M006-E split. Staying inside M006-A is a correctness requirement,
  not timidity.
- **Environment:** verification expects Rust 1.89 and the workspace toolchain. Per the M005
  corrective record, a default x86_64 toolchain may fail to link against arm64-only MacPorts
  libraries; use the arm64 toolchain for local Rust verification and rely on hosted CI for
  the supported runner. Do not claim local runs that did not happen.
- **Serial tests:** repository tests mutate process-global environment; use the `ci` nextest
  profile (one process per test) for broad sweeps. New `#[tokio::test]`s default to
  `current_thread`.
- **Storage tests:** use `isolated_pool()` where a pool is needed; migrations run inside it.
- **Preserve unrelated user changes.** The baseline had a clean working tree; if that is no
  longer true, do not revert anything you did not author.
- **No `--all-features`.** It pulls in real-LSP-server tests. Broad sweeps use
  `scripts/verify.sh full`, which pins `--features server,plugins,lsp-test-support`.
