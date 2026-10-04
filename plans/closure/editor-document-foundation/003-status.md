# Shared Editor Document Foundation M005-C — Closure Status

Status: closed

Source implementation plan: `plans/implementation/editor-document-foundation/003-lsp-sync-checked-save-and-external-conflict.md`

Source subsystem roadmap: `plans/subsystems/editor-document-foundation-roadmap.md#milestones`

Repository baseline reviewed: `b39f7959`

Implementation commits:

- `420ba3ce` — managed LSP synchronization, checked save/reload, external conflict reconciliation
- `62df8477` — serialize document changes and preview applies through per-document operation gates
- `ebac1fb4` — pass canonical document service through model preview apply

## 1. Executive finding

M005-C is complete. Canonical daemon document state now synchronizes to `egglsp`
as managed text, checked save uses the existing workspace lock authority and
disk-base digest, external changes refresh clean snapshots or conflict dirty
ones, and dirty LSP previews fail before disk mutation. `DocumentService`
remains authoritative; LSP and disk mutations never roll back accepted editor
text.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Managed versus disk-owned LSP source | `OpenDocumentSource`, managed registry snapshots | pass | Source and desired text survive replay; the disk refresh helper preserves managed text. |
| Accepted edits synchronize full text | `CoreDaemon::handle_document_request`, `set_managed_document` | pass | Response includes `lsp_degraded`; registry records desired text before transport delivery and retries stale state before semantic preparation. |
| Save uses canonical mutation authority | `checked_workspace_text_write`, workspace lock and path validators | pass | Existing regular UTF-8 files only; rereads SHA-256 under lock; sibling temporary file and parent are synced before return. |
| Save conflict leaves disk and editor text unchanged | checked-write conflict test; `DocumentService::mark_disk_conflict` | pass | Mismatch writes nothing and returns structural digest metadata. |
| Save/edit concurrency is serialized | `accepted_change_waits_behind_checked_save_operation_gate` | pass | Barrier/oneshot-forced test proves a change waits behind the save operation gate. |
| External write versus save is deterministic | `external_writer_waits_for_checked_save_workspace_lock` | pass | Workspace serialization makes save complete before the queued external writer. |
| Clean and dirty external change policy | `external_disk_change_refreshes_clean_text_and_preserves_dirty_text_as_conflict` | pass | Snapshot/open focus boundary refreshes clean state with a new revision; dirty state preserves exact text and becomes conflicted. |
| Reload authorization and revision fence | `DocumentReload` handler and `reload_verified` | pass | Requires `file.modify`, writer lease, safe path, bounded UTF-8 disk text, and expected revision. |
| Dirty LSP preview cannot mutate disk | `dirty_document_service_rejects_preview_before_disk_write` | pass | Both daemon and model-facing preview apply hold canonical document operation gates before the workspace lock. |
| Agent file tools remain disk-authoritative | `architecture/tool.md`, existing tool mutation paths | pass | Tools are not redirected to unsaved document text. |

## 3. Production implementation evidence

`egglsp` records `Disk` and `Managed` sources, desired text, dirty state, and a
`sync_stale` marker. Managed text is recorded before didOpen/didChange; delivery
failure leaves it available for replay and semantic retry. The legacy disk
helper refreshes only disk-owned files. Document open, change, snapshot,
reload, and save paths synchronize canonical full text. didSave follows the
checked disk commit.

Checked save/reload and preview apply share the workspace repository lock.
The checked writer canonicalizes the root, revalidates a workspace-relative
regular file, checks the on-disk SHA-256, syncs a sibling temporary file,
atomically renames, and syncs the parent directory. New-file creation remains
out of scope. Preview apply keeps its existing multi-file checkpoint,
rollback, and post-state semantics.

External disk changes are checked at explicit open/snapshot boundaries and
before save. A clean buffer refreshes from bounded UTF-8 disk text and advances
revision. A dirty buffer is preserved and marked conflicted. No watcher is
required for correctness; notifications remain advisory.

## 4. Verification executed

### Commands run

```bash
rtk cargo fmt --all
rtk cargo test --target aarch64-apple-darwin -p egglsp --locked --features lsp-test-support
rtk cargo test --target aarch64-apple-darwin --features lsp-test-support \
  --test document_lsp_integration --test lsp_composite_stdio \
  --test edit_checkpoint_integration --test checked_restore_integration \
  -- --test-threads=1
rtk cargo test --target aarch64-apple-darwin -p codegg document_service::tests --lib
rtk cargo test --target aarch64-apple-darwin -p codegg lsp::mutation::tests --lib
rtk cargo clippy --workspace --all-targets --locked -- -D warnings
rtk scripts/verify.sh quick
rtk git diff --check
```

### Results

The egglsp suite passed 1,001 unit tests and all package integration targets;
its doctest pass was rerun with the Rust 1.89 `RUSTDOC` explicitly selected
after the host rustdoc could not find the cross-target standard library. The
four root integration targets passed (82 tests). The focused root document
service and LSP mutation suites passed, including the checked-write, clean/
dirty conflict, save/edit barrier, workspace-lock race, dirty-preview, and
managed LSP stdio cases. Host-native workspace clippy and `verify.sh quick`
passed. The cross-target clippy probe hit a third-party `cpufeatures`/`libc`
target build error; the repository-native clippy command passed with
`-D warnings`.

## 5. Invariant review

- `DocumentService` remains the sole canonical text/revision authority.
- LSP stores a replayable mirror and never replaces managed text from disk.
- Disk is durable authority; a save writes only when the recorded digest still matches.
- Save/reload/change and checked preview apply use per-document serialization;
  disk mutations also share `WorkspaceLockTable`.
- Clean external changes refresh only at explicit open/snapshot boundaries;
  dirty text remains intact and conflicted.
- File bodies are excluded from audit and conflict metadata.
- Agent file tools remain disk-authoritative; new-file creation and watchers
  are deferred.

## 6. Failure and recovery review

Accepted text survives LSP failure and the client receives a bounded degraded
flag. The registry retains desired text and retries it on the next semantic
preparation or document synchronization. Save mismatch writes nothing and
keeps the buffer dirty/conflicted. A newer document change cannot cross an
in-flight save or preview apply gate. LSP preview failure leaves the candidate
available for an explicit fresh attempt.

## 7. Migration and compatibility review

The protocol remains additive and no storage migration was introduced. Save
and reload replace the former typed not-ready response. Existing disk-owned
LSP callers retain their refresh behavior. Document responses gained explicit
LSP degradation metadata; no document body is added to event or audit records.

## 8. Security review

Document authorization runs before file access. Save, reload, and writer
acquisition require `file.modify`; reads require `file.read`. All checked file
paths are workspace-relative and canonicalized under the workspace root;
symlink targets and non-regular files are rejected. Disk and inserted text
remain bounded. Conflict messages contain hashes and structural locators only.
The audit matrix classifies `document_modify` as `file_mutate` and documents
bounded document reads as intentionally uninstrumented.

## 9. Documentation and operations

Updated `architecture/document.md`, `architecture/lsp.md`,
`architecture/workspace_services.md`, `architecture/tool.md`,
`architecture/protocol.md`, and `architecture/audit.md` alongside the
implementation. `DocumentStatusGet` remains the project-authorized polling
surface from M005-B; this milestone adds no global document event fanout.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| none | No unresolved M005-C finding | none | none |

## 11. Roadmap disposition

M005-C is closed. M005-D is unblocked to ready: the stable protocol now has
canonical text, revision, writer, conflict, save, reload, LSP degradation, and
resynchronization semantics for a reusable client replica and TUI adapter.
The parent M005 remains open until M005-D qualification completes.

## 12. Registry updates

- Mark M005-C closed and remove it from dependency-ready work.
- Mark M005-D ready and remove the strict M005-C closure blocker.
- Keep M006 deferred until strict M005-D/parent M005 closure.
