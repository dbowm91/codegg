# Editor Documents

The shared editor document foundation is frontend-neutral. `codegg-document`
owns immutable text snapshots and deterministic edits; the daemon service owns
open document identity, authorization, attachments, revisions, and disk-base
state; `codegg-client` owns an optimistic replica. Disk remains the durable
source of truth, and the LSP registry is a semantic mirror. Agent file tools
remain disk-authoritative.

## Text and transaction contract

`codegg-document` uses `crop` 0.4.3 behind a private backend boundary. It was
selected over `ropey` 1.6.1 because it supports byte-addressed rope edits,
immutable cheap clones, UTF-8 boundary checks, and indexed line lookup, and
declares Rust 1.85 MSRV (below this workspace's Rust 1.89). The crate does not
expose either backend type. Canonical ranges are sorted, disjoint UTF-8 byte
ranges against the pre-transaction snapshot. Edits apply in reverse order;
inverse edits are returned in ascending post-image coordinates. Revisions begin
at zero, empty transactions do not advance revisions, and line endings are
preserved exactly.

The core accepts UTF-8 strings including NUL; editor-service policy owns file
classification and resource bounds. LSP-specific UTF-16/UTF-32 positions remain
owned by `egglsp`; this crate only maps byte offsets and byte columns.

## Daemon protocol

The additive `document.v1` native surface is owned by the root daemon's
process-local `DocumentService`. Open keys include project, workspace, and a
normalized workspace-relative path. Opening attaches read-only with `file.read`;
writer acquisition is a separate `file.modify` operation. The daemon validates project/workspace
binding, performs file capability authorization before the handler reads text,
rejects path traversal and symlink components, and bounds snapshots to 8 MiB.
One transport connection owns the writer lease; disconnect releases its
attachments and lease but keeps dirty canonical text. Change requests carry a
stable id and base revision, and exact duplicate ids return their accepted
revision. Conflicting payload reuse and stale revisions fail without mutation.
Observers use authorized `DocumentStatusGet` revision polling. Document events
are not sent through the global CoreEvent fanout because that fanout lacks a
project access filter.

Save and reload use workspace-locked checked disk mutation. The client keeps
its replica dirty until an acknowledged save; a typed disk conflict preserves
the replica for explicit reload or recovery.

Unsaved editor text is process-local and may be lost on daemon restart. Save
must compare the current disk digest with the document's recorded disk base
under the existing workspace mutation authority. A mismatch preserves dirty
text and becomes a conflict. The client must not automatically replay divergent
local drafts after reconnect.

## Client replica and TUI boundary (M005-D)

`codegg-client::DocumentController` uses `codegg-document` for rope-backed
optimistic edits. It keeps daemon revision separate from local text, assigns a
stable UUID change ID to each queued transaction, serializes flushes, retains
an uncertain request for idempotent retry, and bounds the queue to 128
transactions / 4 MiB of inserted text. `apply_local` updates the local
snapshot synchronously; one controller-owned debounce task flushes the queue.
Adjacent unsent pure insertions coalesce only when their byte coordinates
prove the merge; all other transactions keep their original boundaries.
Reconnect accepts a replacement transport, reattaches the same scoped file,
reacquires its writer lease, then compares server text/revision against queued
transaction boundaries. It acknowledges a proven accepted prefix, retries an
unchanged base with the same IDs, or retains the draft in resync-required
state. A missing/new document identity becomes `GoneWithLocalDraft`.
The controller has explicit synced, local-dirty, flushing, conflict,
disconnected, resync-required, read-only, and gone-with-draft states. It has no
operational transform: unexpected divergence is retained and surfaced for
recovery.

The TUI `TuiDocumentSession` is a thin ownership seam over that same
controller. It stores only presentation cursor, selection, and viewport
placeholders; it owns no text buffer. Headless and TUI-kind native clients use
the same `document.v1` controller contract. Status polling is metadata-only;
an observer resnapshots when the daemon revision changes.

M006-A gives that seam a production consumer: the TUI editor primary view
(`Route::Editor`). The seam is unchanged in shape — one controller, one
presentation record — but the presentation record now carries cursor, anchor,
viewport line and column, buffer mode, a bounded command prefix, and a
depth/byte-capped undo/redo history. It still holds no text. The render path
reads the replica through two read-only accessors on the controller,
`try_snapshot` (a `DocumentSnapshot` borrow, dropped with the frame) and
`try_attachment_info` (line count, writer lease, conflict flag, no text), so a
frontend can hold the second across frames without holding a second buffer.
`scripts/check_tui_editor_text_authority.py` enforces this statically; see
`architecture/tui.md` for the route, focus model, and viewport contract.

M005 limitations: one writer per document; local drafts are process-local and
not persisted; agent reads remain disk-authoritative; no CRDT/OT is included;
LSP preview apply into a dirty buffer remains rejected. A
daemon restart may orphan a local draft, which is never automatically written
over newly opened disk state.

M006-A does not relax the dirty-buffer rejection. That is M006-E's product
decision, and it requires an ADR; the existing rejection is correct as
written.

M006-A also leaves two M006 limitations standing and does not claim them away.
A GUI editor is still out of scope, and the TUI editor reads no LSP state:
`document.v1` has no LSP read operation, so diagnostics, completion, and
navigation are still reachable only from the agent tool path. That gap is
M006-B and needs an ADR covering the delivery path and project-scoped
authorization.

## LSP, save, and external disk changes

`egglsp` records each open document as `Disk` or `Managed`. Document service
open/change/snapshot/reload calls synchronize a full managed snapshot. A
disk-oriented `ensure_file_open_from_disk` call never replaces managed text;
it retries a stale managed mirror from the registry before semantic work.
Mirror failure is reported as `lsp_degraded`; it never rejects or rolls back
canonical editor text.

`DocumentSave` holds the document operation gate and workspace repository lock,
revalidates the relative target, rereads its SHA-256, and writes through a
synced sibling temporary file plus atomic rename. A disk-base mismatch writes
nothing and marks the buffer conflicted. `DocumentReload` requires the current
writer lease and expected revision. An explicit snapshot boundary detects
external disk changes: clean buffers refresh and advance revision; dirty
buffers keep their exact text and become conflicted. Filesystem notifications
are advisory; tools continue to read and mutate disk through their existing
authorities. A dirty editor document rejects LSP preview apply until saved and
the preview is regenerated. Daemon and model-facing preview apply paths take
clean-document operation gates before the workspace lock, closing the edit
race between their dirty check and multi-file write.
