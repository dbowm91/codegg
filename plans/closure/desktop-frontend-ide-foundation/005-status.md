# Desktop Frontend and IDE Foundation M006-A — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/desktop-frontend-ide-foundation/005-editor-buffer-vertical-slice.md`

Source subsystem roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#m006--tui-ide-vertical-slice`
- `plans/subsystems/desktop-frontend-ide-foundation-m006-tui-presentation-audit.md`

Repository baseline reviewed: `614e983e5a4f57718ac65d1ecee021719500fd75`

Implementation commits or pull requests: none — the work is uncommitted in the
working tree at the time of this record. Proposed commit scope is in section 12.

## 1. Executive finding

M006-A delivers a TUI editor buffer over the existing M005
`DocumentController`: a non-modal `Route::Editor` primary view, a presentation
model, a render widget, and vi-style motion, editing, undo/redo, and save. It
adds no protocol surface, no daemon capability, and no LSP read path.

The milestone's capability boundary is complete for the scope it set. The
status is *conditionally* closed rather than closed because of two named
outstanding items: a deviation from the source plan that the plan itself did
not anticipate, and hosted evidence that cannot be produced in this
environment.

Four defects were found by the milestone's own tests and fixed before
handoff. Two of them were real product bugs that a purely green-field
implementation would have shipped; they are recorded in section 6.2.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Presentation model holds no text | `scripts/check_tui_editor_text_authority.py`; `tui::editor::tests::*` | pass | Guard is non-vacuous: it asserts the types it reasons about still exist |
| Render reads only the controller replica | `EditorWidget` takes `&DocumentSnapshot` scoped to one frame; the guard's `borrowed-document-field` note is the only document-shaped field in the path | pass | No `Vec<Line>` is ever built |
| Every mutation goes through `apply_local` | `submit_edit`, `editor_undo`, `editor_redo`, `editor_insert_literal`, `editor_delete_backward` all call `controller.apply_local` | pass | No other text-mutating path exists in the editor |
| Undo is a frontend-local inverse transaction | `editor::invert` mirrors `DocumentBuffer::apply`; `inverse_matches_the_controllers_own_inverse_for_multi_edit_transactions` | pass | Frontend and controller inverses are asserted equal |
| Undo is depth- and byte-capped | `undo_history_drops_the_oldest_entry_past_the_depth_cap`, `undo_history_drops_the_oldest_entry_past_the_byte_cap` | pass | 256 entries / 4 MiB across both directions |
| Undo does not survive reconciliation | `undo_history_does_not_survive_reload_resync_or_close` | pass | Save deliberately preserves history; every other transition clears it |
| Redo is exact, not a re-undo | `editor_keeps_edit_through_undo_redo_save_and_reopen` | pass | Found and fixed a real defect; see 6.2 item 2 |
| Route, focus, and layout branch | `Route::Editor`; `focus_moves_between_the_buffer_and_the_composer_without_touching_text`; `unclaimed_keys_fall_through_to_the_composer` | pass | Composer stays editable; matches the `Route::Workspace` precedent |
| Bounded open path with project authority | `accepts_a_plain_workspace_relative_path`, `rejects_absolute_paths`, `rejects_traversal_components`, `rejects_empty_and_degenerate_paths`, `rejects_over_long_and_over_deep_paths` | pass | Lexical check only; the daemon remains the containment authority |
| Every `DocumentState` and error has distinct presentation | `every_controller_state_maps_to_a_distinct_status`, `every_controller_error_maps_to_a_distinct_notice_message` | pass | Distinct labels and messages are asserted, not merely non-empty |
| Per-frame work bounded by the viewport | `per_frame_work_is_bounded_by_the_viewport_not_the_document` (20k lines), `a_very_long_line_is_truncated_to_the_viewport` | pass | |
| Partial open failure shows no buffer | `a_failed_open_holds_no_attachment_and_no_buffer`, `an_unexpected_open_response_holds_no_attachment` | pass | No attachment, no snapshot, no chrome |
| Read-only refuses every edit | `a_read_only_attachment_renders_but_refuses_every_edit` | pass | Text still renders; no history is recorded |
| Serial change submission under a burst | `a_rapid_edit_burst_serializes_with_one_change_in_flight` | pass | 25 edits, max observed concurrency 1 |
| Destructive phase refuses local edits | `a_local_edit_during_a_destructive_phase_is_refused_and_history_survives` | pass | Real concurrency, `multi_thread` runtime |
| Conflict retains and surfaces the draft | `a_disk_conflict_retains_and_surfaces_the_draft_without_resolving_it` | pass | No auto-reload, no auto-discard |
| Gone document surfaces recovery state | `a_gone_document_surfaces_a_recovery_state_and_keeps_the_draft` | pass | |
| Switching documents drops the previous attachment | `switching_documents_clears_history_and_drops_the_previous_attachment` | pass | |
| Guard fails on a violating construct | Four demonstrations, section 4.3 | pass | Reverted after each |
| Existing TUI behavior unchanged when the editor is closed | `/editor` still opens the external editor; the TUI buffer is `/open` | pass | No existing binding or route was repointed |
| Workspace clippy is clean | `cargo clippy --workspace --all-targets --locked -- -D warnings` | pass | Two findings raised and fixed first; final run emits nothing |
| `verify.sh quick` routine subset green | `bash scripts/verify.sh quick`, exit 0 | pass | Includes the new editor guard |
| Hosted CI green | `.github/workflows/ci.yml` `verify` job | not run | Environment limit; see 4.4 |

## 3. Production implementation evidence

**New frontend modules.**

- `src/tui/editor.rs` — the presentation model. Boundary-safe offset helpers
  over `DocumentSnapshot`, motion resolution, transaction construction and
  inversion, the bounded undo/redo history, and the hard-wrap viewport
  calculation. Holds no text.
- `src/tui/app/state/editor.rs` — `EditorState` plus the presentation of
  controller states (`EditorStatus`) and errors (`EditorNotice`, with
  severity). Holds no text.
- `src/tui/components/editor.rs` — the render widget: line-number gutter,
  current-line highlight, horizontally scrolled viewport, state banner, and a
  mode/focus title.
- `src/tui/commands/editor.rs` — the command layer: open, close, save, reload,
  resync, both completion handlers, and the buffer key handler, plus the
  trajectory test suite over a scripted `document.v1` transport.

**Wiring.** `Route::Editor`; `TuiTaskKind::Editor`; `TuiCommand::EditorOpened`
and `TuiCommand::EditorOperationFinished`; dispatch arms in
`src/tui/runtime/command_dispatch.rs`; the `on_key` hook in
`src/tui/app/mod.rs`; `render_editor_view` and an editor header title in
`src/tui/app/render.rs`; `EditorState` on `App`; the `/open <path>` builtin;
and the editor binding set in the help overlay.

**`codegg-client` addition (deviation).** `DocumentController::try_snapshot()`
and `DocumentController::try_attachment_info()`, plus the
`DocumentAttachmentInfo` type. `snapshot()` is now a thin wrapper over
`try_snapshot()`. Both accessors are read-only, mutate nothing, allocate no
protocol state, and mirror the synchronous `apply_local` accessor the TUI
already depended on. See section 10.

**Deliberately absent.** Soft wrap; a yank/paste register; visual selection
commands; syntax highlighting; any LSP read surface. None is in M006-A's
scope. `selection` is owned by the presentation record and is boundary-safe,
but no M006-A command sets it; visual selection is deferred.

## 4. Verification executed

### 4.1 Commands run

```bash
cargo fmt
cargo test --lib tui::                                                  # 1045 passed
cargo test -p codegg-document --locked                                  # 5 passed
cargo test -p codegg-client --locked                                    # 22 passed
TMPDIR=/tmp cargo test -p codegg-client --test gui_client --locked      # 9 passed
cargo test --test tui_render --locked                                   # 99 passed
cargo test --test document_client_trajectory --locked                   # 3 passed
cargo clippy --workspace --all-targets --locked -- -D warnings
python3 scripts/check_tui_editor_text_authority.py
bash scripts/verify.sh quick
```

### 4.2 Results

- `cargo fmt` — clean.
- `cargo test --lib tui::` — **1045 passed, 0 failed** in 3.15s.
- `cargo test -p codegg-document --locked` — 5 passed.
- `cargo test -p codegg-client --locked` — 22 passed (lib).
- `TMPDIR=/tmp cargo test -p codegg-client --test gui_client --locked` — 9
  passed. See 4.5.
- `cargo test --test tui_render --locked` — 99 passed.
- `cargo test --test document_client_trajectory --locked` — 3 passed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` —
  **clean**; the final run emits no errors or warnings. Two findings were
  raised and fixed first: `field_reassign_with_default` in the trajectory
  fixture, and `reversed_empty_ranges` on the intentionally inverted
  selection fixture.
- `bash scripts/verify.sh quick` — **passed, exit 0**. Every guard in the
  routine subset reported green, including `check_tui_project_authority.py`,
  `check_execution_ownership.py`, `check_scheduler_bypass.py`,
  `check_http_route_disposition.py`, `check_audit_coverage.py`, and the new
  `check_tui_editor_text_authority.py`, followed by
  `cargo check --workspace --all-targets --locked`.

### 4.3 Guard demonstrations

The static guard was shown to fail and then to pass again for each violation
class it claims to catch. Each construct was injected, the guard run, and the
change reverted.

| # | Injected construct | Guard finding | Exit |
|---|---|---|---|
| 1 | `text: String` field on `TuiDocumentPresentation` | `retained-text-field:text` | 1 |
| 2 | `snapshot: DocumentSnapshot` (owned) on `EditorState` | `retained-document-field:DocumentSnapshot` | 1 |
| 3 | `std::fs::read_to_string` in the open command | `filesystem:read_to_string`, `filesystem:std::fs` | 1 |
| 4 | `DocumentBuffer::new` in the editor model | `buffer-owner:DocumentBuffer::` | 1 |
| — | none (reverted tree) | pass | 0 |

Demonstration 2 is what proves the owned/borrowed distinction is real: the
sanctioned `&DocumentSnapshot` borrow in `EditorWidget` is reported as an
advisory note and does not fail the guard, while the owned field does.

### 4.4 Not run, and why

- **Hosted CI `.github/workflows/ci.yml` `verify` job** — not run; this
  environment has no hosted runner. Local coverage of the same checks is in
  4.2, and the new guard is registered in both `verify.sh quick` and the CI
  job, so the hosted run will include it. The Desktop E2E workflow was
  likewise not run; M006-A does not touch `apps/desktop`, so that surface is
  unchanged.
- **`cargo nextest run --workspace --locked --profile ci`** — not run. The
  targeted invocations in 4.1 plus workspace clippy and `verify.sh quick`
  cover the changed surface, but a full capped sweep remains outstanding
  before the milestone is marked closed.

### 4.5 Environmental failure, unrelated to this milestone

`cargo test -p codegg-client --test gui_client` fails 7 of 9 tests on this
machine with `path must be shorter than SUN_LEN`. The cause is
`std::env::temp_dir()`: this session's `$TMPDIR` is
`/var/folders/2j/dlwhrpps66scv9bw8f7vdfg40000gq/T/` (49 bytes), and the tests
append `codegg-client-<36-byte-uuid>.sock`, exceeding the 104-byte macOS Unix
socket path limit. Re-running with `TMPDIR=/tmp` passes all 9. No M006-A code
runs in that failure path; the panic is on the tests' first setup line.

## 5. Invariant review

| Source-plan invariant | Evidence |
|---|---|
| 1. The TUI holds no second text buffer | Guard passes; `EditorState`, `TuiDocumentPresentation`, and `EditorUndoEntry` hold only bounded inverse transactions. The only document-shaped item in the path is a frame-scoped borrow. |
| 2. The editor renders from `DocumentController::snapshot()` | `render_editor_view` calls `session.try_snapshot()` and drops the snapshot with the frame; `EditorWidget` borrows it |
| 3. Every mutation goes through `apply_local` | All six mutating helpers call it; the burst test proves one change in flight at a time |
| 4. Undo is frontend-local and bounded | `editor::invert`; depth/byte caps with tests; no undo request on the wire |
| 5. Undo does not survive reconciliation | `EditorOperation::clears_undo_history` plus `undo_history_does_not_survive_reload_resync_or_close` |
| 6. The editor never reads files directly | The guard's filesystem rule; the open command's lexical check performs no I/O |
| 7. Project authority is explicit | `open_editor` reads `app.project_execution_context()` and refuses when project or workspace is absent; `check_tui_project_authority.py` stays green |
| 8. `document.v1` is frozen | No protocol, DTO, or `CoreRequest` change. `codegg-protocol` has no diff. |
| 9. Async calls use `spawn_tui_task` with a completion guard | Open, save, reload, resync, and close all spawn `TuiTaskKind::Editor` tasks carrying a request id and a generation; mismatched generations are discarded in both apply handlers |
| 10. A new guard protects the editor's text authority | `scripts/check_tui_editor_text_authority.py`, registered in `verify.sh quick` and CI, with four demonstrated failures |
| 11. Hard wrap keeps line-to-screen one-to-one | `resolve_viewport`; gutter-stability and viewport-clamp tests |
| 12. The existing dirty-buffer rejection is unchanged | `src/lsp/mutation.rs` is not in the diff; M006-E owns that decision |

## 6. Failure and recovery review

### 6.1 Coverage

- **Duplicate delivery / idempotency**: `a_second_open_of_the_same_document_is_refused_not_silently_orphaned`; the controller's `AlreadyOpen` behavior is unchanged.
- **Cancellation races**: not exercised at the editor layer. Generation guarding is asserted structurally — a mismatched generation returns before touching state — rather than by a cancellation test. See section 10.
- **Daemon restart / transport loss**: `a_divergent_resync_keeps_the_draft_and_clears_undo`; the M005 controller recovery tests remain green. M006-A fabricates no draft.
- **Partial persistence failure**: `a_failed_open_holds_no_attachment_and_no_buffer`; `an_unexpected_open_response_holds_no_attachment`.
- **Stale generation**: `EditorState::detach` bumps the generation and cancels the request; `detach_invalidates_in_flight_completions`.
- **Contention**: `a_rapid_edit_burst_serializes_with_one_change_in_flight`; `a_local_edit_during_a_destructive_phase_is_refused_and_history_survives`.
- **Malformed / unauthorized input**: the five path-rejection tests.
- **Bounded behavior**: viewport-bounded render; depth/byte-capped undo; a 2-byte pending command prefix; 512-byte and 64-component path bounds. Oversized edits and queue exhaustion are surfaced, never retried internally.

### 6.2 Defects found and fixed during the milestone

1. **Insertion did not advance the cursor.** `build_edit` clamped
   `cursor_after` against the *pre*-edit snapshot, so every inserted character
   landed at the same offset and consecutive keystrokes reversed themselves.
   Caught by `insert_mode_keys_type_into_the_buffer_and_escape_leaves_insert`.
   Fixed by deferring the clamp to the post-edit snapshot.
2. **Redo re-applied the undo.** `EditorUndoEntry::rebased` swapped the forward
   and inverse transactions when moving an entry between stacks. The pair is
   direction-labelled, not stack-labelled, so swapping made redo delete the
   edit. Caught by `editor_keeps_edit_through_undo_redo_save_and_reopen`. Fixed
   by rebasing only the recorded cursor.
3. **`Esc` was unhandled below the App layer.** The buffer key handler reported
   `Esc` as unclaimed, so the composer would have consumed it. Fixed with a
   `BufferKey::RequestLeaveBuffer` outcome and a `leave_editor_buffer` step
   that exits insert mode first, then focus.
4. **`dd` on the last line left a trailing blank line**, and
   `offset_at_column` returned `None` at end-of-document instead of the end
   offset. Both were logic errors in the model, fixed and covered.

## 7. Migration and compatibility review

- **Schema migration**: none. No storage layout change.
- **Protocol negotiation**: unchanged. `codegg-protocol` has no diff.
- **Backward compatibility**: `/editor` still opens the *external* editor; the
  TUI buffer is reached through the new `/open`, so no existing binding changed
  meaning. The built-in command count moved 151 to 152 in `src/tui/command.rs`
  and `architecture/command.md`, with both count assertions updated.
- **Configuration**: no new key. The editor is reachable from any project tab
  with an explicit execution context.
- **Rollback**: reverting the four new frontend modules and the
  `codegg-client` accessors removes the feature completely. Nothing outside the
  TUI depends on it.
- **Legacy path status**: the existing dirty-buffer rejection at
  `src/lsp/mutation.rs` is untouched and still correct.

## 8. Security review

- **Authorization**: the editor opens documents only through `DocumentOpen`, so
  the daemon's `file.read` authorization and workspace containment are the sole
  authority. The frontend's lexical path check is a usability pre-check and
  grants nothing.
- **Path validation**: absolute paths, drive-qualified paths, UNC prefixes,
  `..` traversal, empty components, NUL bytes, over-long paths, and over-deep
  paths are all rejected before a request is issued. The daemon validates
  independently.
- **Privilege boundary**: the open path requests the writer lease, and the
  daemon's one-writer rule decides it. A read-only attachment renders but
  refuses every edit, with an explicit `read-only` banner.
- **Secret handling**: no secret is read, logged, or displayed. The banner
  carries only controller state and surfaced error messages.
- **Denial of service**: per-frame work is viewport-bounded; undo is depth- and
  byte-capped; the pending command prefix is 2 bytes; path length and depth are
  bounded. Oversized edits and queue exhaustion are surfaced, not retried.
- **Audit**: no new audit surface. Document operations already flow through the
  existing audited `document.v1` requests.

## 9. Documentation and operations

**Updated.**

- `architecture/tui.md` — new `Route::Editor` and `Document editor (M006-A)`
  section covering the single-buffer invariant, presentation record,
  reconciliation semantics, focus model, hard wrap, bounded per-frame work,
  async discipline, and project authority. The stale `Routes` block now
  includes `Workspace` and `Editor`. A naming note disambiguates the
  pre-existing "M006" durable-agent-run milestone from the roadmap's M006.
- `architecture/document.md` — records that the TUI seam now has a production
  consumer, restates the M005 limitations that still hold, and names the two
  M006-A does not resolve.
- `architecture/command.md` — `/open` added, count 151 to 152.
- `plans/registry.md` and the subsystem roadmap — M006-A status, evidence, and
  the two gates that hold M006-B, M006-D, and M006-E.
- The M006 audit record's §16 disposition now records the M006-A outcome.

**Static guard.** `scripts/check_tui_editor_text_authority.py`, registered in
`scripts/verify.sh quick` and `.github/workflows/ci.yml`. Re-run with
`python3 scripts/check_tui_editor_text_authority.py`.

**Discovery.** `/?` or the help overlay lists the full editor binding set.
`/open <workspace-relative path>` opens a document. `Ctrl+E` focuses the
buffer; `Esc` walks back out. `Ctrl+S` saves, `Ctrl+R` resyncs, `R` reloads
from disk, `u` undoes, and `Ctrl+R` in normal mode redoes.

**Recovery.** On a disk conflict the editor keeps the draft and says so; `R`
reloads from disk and discards it, which is the only way to take the disk
version. On `resync required`, `Ctrl+R` resyncs. On `document gone` the draft
is retained; closing the editor and reopening is the only way to start fresh.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | Two read-only accessors were added to `codegg-client` (`try_snapshot`, `try_attachment_info`), which the source plan's §16 listed as not-to-edit. The alternative was caching text in the frontend, which the plan's own invariant 1 forbids. The additions are additive, read-only, and semantics-preserving. | The deviation should be reviewed rather than absorbed silently | Confirm or reject in review. If rejected, the alternative is a `document.v1` snapshot read per frame, which is a protocol change and strictly larger. |
| medium | Hosted CI `verify` job not run | Clippy, fmt, and every routine guard are green locally, but the hosted sweep is unconfirmed | Run `.github/workflows/ci.yml` `verify` before marking closed |
| medium | `cargo nextest run --workspace --locked --profile ci` not run | A full capped sweep is outstanding | Run in the same hosted pass |
| medium | Async cancellation is guarded structurally but not tested by a cancellation test | A cancelled mid-flight completion is expected to be discarded by the generation check, but that path is unexercised at the editor layer | Add a stale-generation completion test when the App-level `spawn_tui_task` cancellation harness is next touched |
| low | No yank/paste register, so `dd` is only recoverable via undo | Reduced editing convenience, not a correctness issue | Deferred; the plan did not require paste |
| low | `selection` is owned and boundary-safe but never set by an M006-A command | Dormant field until visual selection lands | Deferred to the visual-selection milestone |
| low | Hard wrap means one very long line is horizontally scrolled rather than wrapped | Long minified or generated lines need horizontal scrolling | Soft wrap deferred, with the reasoning recorded in the plan and `architecture/tui.md` |
| low | `architecture/tui.md`'s "M006" durable-agent-run label and `architecture/ide.md`'s "IDE" meaning remain ambiguous repo-wide | Documentation ambiguity only; a naming note was added at the durable-run section | A user-directed terminology amendment per `plans/003-planning-process.md` §2.1, not a corrective edit |

## 11. Roadmap disposition

**Milestone conditionally closed with named operational evidence outstanding.**

M006-A's capability boundary is complete and its invariants hold. The two
outstanding items are the §10 `codegg-client` deviation review and the hosted
CI plus nextest sweep. Until both are done, M006-B, M006-D, and M006-E must
not be handed off: M006-B adds a protocol and daemon surface on top of the
render path M006-A established, so a rejected deviation would change it.

M006-B still requires an ADR covering the LSP delivery path and project-scoped
authorization. M006-E still requires an ADR if apply-into-dirty-buffer is
chosen; the existing rejection at `src/lsp/mutation.rs` is correct and must not
be weakened.

## 12. Proposed commit scope

One implementation commit plus one planning commit, matching the repository's
convention:

- `feat(tui): add M006-A editor buffer over the document controller` — the
  four new modules, the wiring, the `codegg-client` accessors, the static
  guard, its `verify.sh` and CI registration, the command-count bump, and the
  three architecture documents.
- `plans(desktop-frontend): close M006-A editor buffer conditionally` — this
  record, the registry rows, the roadmap status, and the audit disposition
  update.

The audit record
(`plans/subsystems/desktop-frontend-ide-foundation-m006-tui-presentation-audit.md`)
and the source plan are planning artifacts and belong in the planning commit.
