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
rejects path traversal and symlink components, and bounds snapshots to 8 MiB
(`codegg_protocol::document::MAX_DOCUMENT_TEXT_BYTES`). Per-request bounds
are `MAX_DOCUMENT_EDITS` (256) and `MAX_DOCUMENT_INSERT_BYTES` (1 MiB); the
service also holds at most `MAX_DOCUMENTS` (128) open documents.
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
transactions (`MAX_PENDING_TRANSACTIONS`) / 4 MiB of inserted text
(`MAX_PENDING_INSERT_BYTES`). `apply_local` updates the local
snapshot synchronously; one controller-owned debounce task flushes the queue.
Adjacent unsent pure insertions coalesce only when their byte coordinates
prove the merge; all other transactions keep their original boundaries.
Reconnect accepts a replacement transport, reattaches the same scoped file,
reacquires its writer lease, then compares server text/revision against queued
transaction boundaries. It acknowledges a proven accepted prefix, retries an
unchanged base with the same IDs, or retains the draft in resync-required
state. A missing/new document identity becomes `GoneWithLocalDraft`.
`DocumentState` has 10 explicit variants: `Closed`, `Opening`, `Synced`,
`DirtyLocal`, `Flushing`, `ResyncRequired`, `Conflict`, `ReadOnly`,
`Disconnected`, and `GoneWithLocalDraft`. It has no operational transform:
unexpected divergence is retained and surfaced for recovery. The companion
`DocumentControllerError` enum (`Transport`, `Response`, `Edit`, `NotOpen`,
`AlreadyOpen`, `LifecycleBusy`, `ReadOnly`, `QueueFull`, `QueueBytes`,
`ResyncRequired`, `Conflict`) carries the full failure vocabulary — see
`crates/codegg-client/src/document.rs` for the authoritative listings.

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
`try_snapshot` (an owned `(DocumentSnapshot, u64)` tuple over the
controller's own buffer, dropped with the frame) and
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

The M006-D file tree does not relax it either, and does not touch this
contract at all. The tree is a *navigator*: it enumerates directory entries
and never reads file bytes, and selecting a file routes through the same
`DocumentOpen` path as `/open`. A workspace-relative path from the tree
therefore gets exactly the same containment, `file.read` authorization,
symlink policy, and dirty-buffer handling as a typed one, because it is
literally the same code path rather than a parallel implementation.

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

The TUI's `/review` surface (M006-E) sits in front of that apply and does not
weaken it. A review shows the change destined for a *saved* document and waits
for an explicit accept; a dirty-buffer target is refused by the same daemon
check as before, and the refusal is shown verbatim rather than paraphrased, so
a user who edits after generating a candidate is still told to save and
regenerate. Merging an apply into a *dirty* buffer — rather than refusing it —
is not in scope for M006-E and is deferred to a future ADR; the rejection in
`src/lsp/mutation.rs` is unchanged by that milestone.

## Source verification

Verified 2026-10-06 against `crates/codegg-document/` (manifest and all five
source modules), `crates/codegg-protocol/src/document.rs`,
`src/document_service.rs`, `src/core/daemon_documents.rs`,
`src/lsp/mutation.rs`, `crates/codegg-client/src/document.rs`, and
`crates/codegg-core/src/authorization/policy.rs`.
- Confirmed the crop claims. `crates/codegg-document/Cargo.toml` pins
  `crop = "=0.4.3"` with `default-features = false, features = ["std"]`;
  that release declares `rust-version = "1.85"` against the workspace's
  `1.89` (`Cargo.toml:12`), and `ropey` is absent from `Cargo.lock`, with
  `ropey` 1.6.1 recorded as the rejected alternative in ADR-0011 and
  `plans/closure/editor-document-foundation/001-status.md`.
- Confirmed the private backend boundary. `lib.rs` declares `mod buffer;`
  (not `pub mod`) and re-exports only `DocumentBuffer`, `DocumentRevision`,
  `DocumentSnapshot`, `AppliedTransaction`, `DocumentLimits`, `TextEdit`,
  `TextRange`, `TextTransaction`, `DocumentError`, `Result`, and
  `BytePosition`. `crop::Rope` appears only inside `buffer.rs` / `edit.rs`,
  and only in private or `pub(crate)` fields, so no backend type reaches the
  public API.
- Confirmed the transaction contract in source: revisions start at zero
  (`DocumentRevision::default()`), an empty transaction returns the current
  revision without advancing it (`buffer.rs:158-164`), edits apply in reverse
  (`buffer.rs:192`), inverse edits are built in ascending post-image
  coordinates from a running shift (`buffer.rs:171-191`), and `validate()`
  enforces sorted, disjoint, UTF-8-boundary, and limit rules
  (`edit.rs:56-103`). `DocumentLimits::default()` is unbounded, so resource
  policy stays with the daemon, as this doc states. NUL is accepted: no
  `DocumentError` variant rejects it.
- Confirmed every numeric claim the M005/M006 passes added:
  `MAX_DOCUMENT_TEXT_BYTES` = 8 MiB, `MAX_DOCUMENT_EDITS` = 256,
  `MAX_DOCUMENT_INSERT_BYTES` = 1 MiB (`document.rs:6-8`);
  `MAX_DOCUMENTS` = 128 (`src/document_service.rs:16`);
  `MAX_PENDING_TRANSACTIONS` = 128 and `MAX_PENDING_INSERT_BYTES` = 4 MiB
  (`crates/codegg-client/src/document.rs:21-22`); and `DocumentState`'s 10
  variants plus `DocumentControllerError`'s 11, both in the exact order
  listed above.
- Corrected one accessor description: `try_snapshot` returns an owned
  `(DocumentSnapshot, u64)` tuple (`crates/codegg-client/src/document.rs:272`),
  not a borrow.
- Verified accurate: the `file.read` / `file.modify` split
  (`authorization/policy.rs:108-123` maps `DocumentOpen` / `SnapshotGet` /
  `StatusGet` / `Close` to `Capability::FileRead`, and `WriterAcquire` /
  `Change` / `Save` / `Reload` to `Capability::FileModify`, evaluated before
  the handlers read text); symlink-component and non-`Normal`-component
  rejection (`document_service.rs:784-800`); an exact duplicate change id
  returning its accepted revision while a differing payload collides
  (`document_service.rs:331-337`); `detach_client` releasing a connection's
  attachments and lease (`document_service.rs:645`); and `DocumentSave`
  routed through `checked_workspace_text_write`, which revalidates the path,
  rereads and compares SHA-256, writes a `.codegg-save-<uuid>` sibling with
  `sync_all`, then renames (`src/lsp/mutation.rs:32-77`). `CoreEvent` carries
  no `Document*` variant, confirming the "not sent through the global fanout"
  claim.
