# TUI Editor IDE Presence M006 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-editor-ide-presence/006-debounce-managed-didchange.md`

Source subsystem roadmap:

- `plans/subsystems/tui-editor-ide-presence-roadmap.md#milestone-6--debounce-managed-didchange`

Repository baseline reviewed: `d585e75f895cf41cd2241512d8d7e7c37fcece99`

## 1. Executive finding

M006 is complete. A burst of committed local edits to an already-open managed
document produces one full-text `textDocument/didChange` carrying the latest
text. `didOpen` is not on that timer: a document the server has not opened
takes the immediate path, and that path's completion has already run when the
call returns. Save and close cancel a pending change rather than riding or
waiting for it.

**This is not the open-latency fix.** M001 owns the open deadline and was not
changed here: workspace `DocumentOpen` still returns after document authority
and answers `lsp_sync: Pending`. No 800-line timing is claimed.

The sync witness follows the push, not the edit: it is recorded only for a
revision that was actually delivered, it is raise-only, and a failure clears it
only while that revision is still the newest scheduled one.

## 2. Requirement-to-evidence matrix

| Requirement (plan §13) | Evidence | Result | Notes |
|---|---|---|---|
| A burst of committed edits produces one `didChange` of the latest text | `a_burst_of_committed_edits_pushes_the_latest_text_once` | pass | Three edits inside one interval → exactly one completion, and the registry holds the third text. A per-edit path would report three. |
| `didOpen` is not on that timer | `an_unopened_document_is_pushed_immediately_and_not_put_on_the_timer` | pass | With no owner record, the completion callback has already run when `schedule_managed_document_change` returns — no sleep, no polling. |
| The witness matches the last successful push, not the latest committed revision | `in_flight_push_records_its_revision_not_a_newer_committed_revision` | pass | Pushing N while N+1 is committed records N. |
| A failed push does not leave its revision synced | `failed_current_push_clears_its_sync_witness` | pass | The current revision's witness is cleared. |
| A late failure does not clear a newer witness | `older_failed_push_does_not_clear_a_newer_successful_witness` | pass | Guarded by `is_lsp_publication_revision_current`. |
| Save still awaits its own sync and does not ride the timer | `save_cancels_the_pending_timer_and_does_not_ride_it` | pass | `save_file` cancels the pending change and returns; no delayed completion fires afterwards. The daemon's `DocumentSave` arm still awaits `sync_managed_document`. |
| Close does not record a witness for an undelivered push | `closing_a_document_cancels_its_pending_push` | pass | Zero completions after the interval. |
| No TUI timer performs this sync | guard run, below | pass | The debounce lives in `egglsp` + the daemon; the TUI was not changed by this milestone. |

## 3. Production implementation evidence

`crates/egglsp/src/service.rs`: `schedule_managed_document_change` is the only
debounced entry point. An unopened document takes the immediate
`set_managed_document_for_key` path. An opened one gets a generation-stamped
timer task that replaces and aborts the previous timer for the same
`(client key, URI)`; when it fires it re-checks that it is still current,
takes the per-document sync lock, and pushes through
`set_managed_document_for_key_locked`. A push is attempted at most twice
(`MANAGED_DID_CHANGE_RETRY_DELAY` = 25 ms). `close_file` and `save_file`
cancel a pending change first.

`src/document_service.rs`: `publication_revision` (newest scheduled revision
per document) and `publication_locks` (one push lock per document) are the
ordering state; `note_lsp_publication_revision`,
`is_lsp_publication_revision_current`, and `lsp_publication_lock` expose it.
`record_lsp_synced` is raise-only.

`src/core/daemon_documents.rs`: the `DocumentChange` arm commits the
revision, then schedules the debounced publication and answers
`DocumentChanged` (no `lsp_sync` field — ADR-0065 §7). `schedule_managed_change_publication`
notes the newest revision, takes the publication lock, drops a superseded
revision, and hands the push to `egglsp` with a completion callback that
records or clears the witness for exactly the pushed revision. Save, reload,
create, and snapshot-get still await their own publication.

Interval that shipped: **100 ms**, with a 25 ms single retry. Not
configurable; not shared with the completion debounce.

## 4. Verification executed

All results are LOCAL to this worktree. No hosted CI run was made for this
milestone.

```bash
cargo test -p egglsp --lib -- a_burst_of_committed_edits an_unopened_document_is_pushed_immediately closing_a_document_cancels an_interactive_open_narrows
# 4 passed; 0 failed

cargo test -p egglsp --lib -- save_cancels_the_pending_timer
# 1 passed

cargo test -p codegg --lib -- in_flight_push_records failed_current_push older_failed_push
# 3 passed; 0 failed

cargo test -p codegg --test core_daemon -- document_open_deadline_m001
# 3 passed (M001's deadline tests still green)

CARGO_BUILD_JOBS=2 cargo check -p egglsp --all-targets
# clean
```

`scripts/verify.sh scoped` is the roadmap-level closing sweep and is run once
for M001–M006; it is recorded in the roadmap closeout, not claimed here.

## 5. Not claimed

- No live TUI receipt, no hosted CI result.
- No measured 800-line open, and no claim that this milestone shortened one.
- Partial `didChange` ranges were not implemented; full text is still the wire
  shape, as the plan requires.

## 6. Registry

Closing M006 unblocks no registered successor. It is the last milestone of
`plans/subsystems/tui-editor-ide-presence-roadmap.md`.
