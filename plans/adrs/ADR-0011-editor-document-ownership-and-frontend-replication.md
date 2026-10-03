# ADR-0011: Editor Document Ownership and Frontend Replication

Status: accepted

Date: 2026-10-03

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#25-tui-target-behavior`

Affected subsystem roadmaps:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md`
- `plans/subsystems/editor-document-foundation-roadmap.md`
- `plans/subsystems/tui-project-sessions-roadmap.md` (closed dependency; not reopened)
- LSP architecture under `architecture/lsp.md`

## Context

The desktop foundation M001-M004 is closed and proved that CodeGG can support a second rich frontend without moving durable project/session/execution authority out of the singleton daemon. The next IDE prerequisite is not a GUI widget. It is a shared contract for unsaved source text that the TUI can consume first and a future graphical IDE can consume later.

Today CodeGG has several adjacent capabilities but no canonical editor-document owner:

- `egglsp::OpenDocumentRegistry` tracks per-LSP-client URI, language id, LSP version, text, and dirty state so open documents can be replayed after an LSP server restart.
- `LspService::{open_file,update_file,save_file,close_file}` emit the normal text-document lifecycle to the language server.
- `LspService::ensure_file_open_from_disk` intentionally rereads disk and updates an already-open LSP document. That is correct for disk-oriented source-intelligence calls but is unsafe as an editor primitive once CodeGG owns unsaved text.
- LSP position encoding is already negotiated as UTF-8, UTF-16, or UTF-32 and CodeGG has canonical conversion helpers.
- daemon-owned workspace services already provide `WorkspaceLockTable`, explicit workspace identity, path policy, snapshot/checkpoint primitives, checked restore, and secure atomic-write patterns.
- LSP preview application already performs canonical containment checks, pre-state hashing, repository serialization, atomic writes, post-state capture, `FileChanged` publication, and LSP synchronization.
- agent/model file tools remain disk-oriented and their durable edit history is tied to filesystem mutations, not an unsaved editor buffer.
- frontend state today has no shared document replica abstraction.

ADR-0010 deliberately deferred this question and required a later versioned document/buffer contract before an IDE could treat unsaved text as authoritative input to LSP or frontends.

The product direction for the next line is TUI-first: CodeGG should build foundational editor/document components that the TUI IDE can use first. Graphical IDE work is long-term and must not drive the M005 dependency graph.

Research snapshot used for this decision (2026-10-03):

- `crop` 0.4.3: UTF-8 rope, logarithmic edits, cheap copy-on-write clones/snapshots; current docs at https://docs.rs/crop/latest/crop/
- `ropey` 1.6.1: mature UTF-8 rope with Unicode-scalar indexing; current docs at https://docs.rs/ropey/latest/ropey/
- `tui-textarea-2` 0.13.2: maintained Ratatui editor widget with Ratatui 0.30 compatibility; current docs at https://docs.rs/tui-textarea-2/latest/tui_textarea/

The rope/widget research informs implementation selection only. No third-party editor widget becomes a CodeGG authority boundary.

## Decision drivers

- Make the TUI the first IDE frontend without making TUI widget state canonical.
- Preserve a clean future path for a graphical frontend without designing around Monaco/Tauri now.
- Keep unsaved source text out of LSP-internal caches as the primary authority.
- Reuse CodeGG's existing workspace/path/mutation authority rather than add another filesystem owner.
- Preserve stable, explicit project/workspace identity; never infer editor authority from cwd.
- Make save conflicts fail closed when disk changes underneath a dirty buffer.
- Avoid CRDT/OT complexity until multi-writer collaborative editing is a real product requirement.
- Keep ordinary agent/model file tools disk-authoritative in the foundation phase.
- Preserve root Rust 1.89 and avoid a custom rope implementation.
- Keep cursor/selection/viewport/modal key state frontend-owned.
- Permit optimistic local editing without one daemon round trip per rendered keystroke.

## Considered options

### Option A — TUI widget owns the editor buffer

The TUI could embed a text-area widget and treat its in-memory text as the editor truth.

This is rejected as the architectural owner. It would make later frontends either duplicate or bypass TUI-specific state, gives the daemon/LSP no stable revision contract, and couples editor correctness to presentation lifetime.

A TUI widget remains acceptable as a view/input adapter over the shared document contract.

### Option B — `egglsp::OpenDocumentRegistry` becomes the editor authority

This would reuse existing text/version/dirty storage.

It is rejected. The registry is keyed and shaped around LSP client lifecycle/restart replay, its version is an LSP synchronization version rather than a CodeGG document revision, and disk-oriented `ensure_file_open_from_disk` can overwrite the LSP mirror with disk content. LSP must remain a mirror of editor truth, not own it.

### Option C — Frontend-neutral daemon document service with optimistic frontend replicas

The daemon owns each open document's canonical ephemeral text/revision and its relationship to the durable disk base. Frontends maintain bounded optimistic replicas and presentation-local cursor/selection/viewport state. LSP mirrors daemon document snapshots. Saves are checked against the recorded disk base.

This is selected.

### Option D — Durable collaborative document store/CRDT now

This could support concurrent multi-user live editing and daemon-restart recovery of unsaved buffers.

It is rejected for M005. It adds persistence schema, compaction/tombstone semantics, conflict resolution, multi-principal editing authority, and recovery policy before CodeGG has proven a single-editor IDE surface.

## Decision

### 1. The daemon owns canonical open-document text

A daemon-owned `DocumentService` is the canonical owner of currently open CodeGG editor documents.

A document is identified by explicit CodeGG scope plus workspace-relative path, not by cwd:

```text
DocumentKey
  project_id
  workspace_id
  relative_path
```

The service retains:

- canonical UTF-8 text;
- monotonic `DocumentRevision`;
- disk-base content hash;
- dirty/clean state;
- typed external-disk conflict state;
- language/document metadata required to synchronize LSP;
- bounded attachment/client bookkeeping where needed.

The service is ephemeral in M005. It does not persist unsaved buffer bodies to SQLite or another durable journal. Daemon restart invalidates unsaved editor state; recovery persistence requires a future explicit decision.

### 2. A pure document/text crate owns edit semantics

A leaf crate, provisionally `codegg-document`, owns the frontend-neutral text/edit model:

- UTF-8 text storage abstraction;
- byte-offset positions and ranges;
- `DocumentRevision`;
- bounded `TextEdit` / `TextTransaction`;
- validation of sorted/non-overlapping edits and UTF-8 boundaries;
- deterministic transaction application;
- immutable snapshots/cheap cloning;
- line/byte position helpers;
- conversion seams needed by LSP/client adapters;
- optional inverse-transaction support sufficient for frontend-owned undo/redo.

It does not know about:

- CodeGG sessions/projects/authorization;
- filesystem paths or writes;
- LSP process lifecycle;
- TUI/Ratatui;
- Tauri/Monaco;
- agents/providers.

The first implementation must qualify a maintained rope dependency rather than write a custom rope. `crop` is the leading candidate because byte indexing matches the canonical transaction coordinate system and cheap copy-on-write clones fit snapshots; `ropey` is the fallback if qualification finds an MSRV/semantics/performance problem.

### 3. Canonical edit coordinates are UTF-8 byte offsets

CodeGG document transactions use UTF-8 byte offsets.

Every range must:

- be in bounds;
- start/end on UTF-8 code-point boundaries;
- be ordered;
- not overlap another edit in the same transaction.

Line/column, grapheme-aware frontend motion, and LSP UTF-8/UTF-16/UTF-32 offsets are adapter concerns.

The document core may provide helpers, but frontend cursor semantics are not protocol authority.

### 4. Frontends own presentation state and optimistic replicas

The TUI and future GUI own:

- cursor(s);
- selections;
- viewport/scroll position;
- modal editing mode;
- keymaps;
- folding/display decorations;
- panel/layout state;
- frontend undo/redo policy.

A reusable `codegg-client` document controller maintains an optimistic local replica of canonical text/revision so typing does not wait for a daemon round trip before rendering.

The daemon remains final revision authority. A stale base revision produces a typed stale/resync response; the client does not silently merge concurrent edits.

### 5. Disk is durable source authority; saves are checked

Opening a document records a cryptographic hash of the disk text used as its base.

A local edit changes the document revision/dirty state but not the disk-base hash.

Save must:

1. resolve the canonical workspace and relative path server-side;
2. authorize `file.modify`;
3. acquire the existing workspace/repository mutation lock;
4. revalidate containment/symlink policy;
5. reread/hash current disk content;
6. compare with the document's recorded disk base;
7. fail with a typed conflict and perform zero writes if the disk changed unexpectedly;
8. otherwise atomically write the canonical document text;
9. update the disk-base hash and clean state only after the write succeeds;
10. emit normal file-change/checkpoint evidence according to existing mutation ownership;
11. synchronize LSP save state.

mtime/size may be used as cheap invalidation hints but are never correctness authority.

### 6. External mutations never silently overwrite a dirty buffer

Agent tools, human shell commands, Git operations, other CodeGG mutation paths, and external programs remain disk-oriented in M005.

When disk content changes for an open document:

- clean document: the service may explicitly refresh/reload through a checked transition;
- dirty document: preserve the unsaved canonical text and mark a typed disk conflict;
- no automatic three-way merge is required;
- no stale disk reread may replace the unsaved LSP/editor text.

`FileChanged` remains an advisory invalidation signal. Hash comparison/revalidation is the correctness mechanism.

### 7. LSP mirrors the document service

`egglsp::OpenDocumentRegistry` remains the authoritative mirror for what CodeGG has successfully synchronized to each language-server client and remains useful for LSP restart replay.

It is not the editor-document authority.

M005 must add an explicit editor-managed synchronization path so that:

- document open -> LSP `didOpen` with canonical document text;
- document change -> `didChange` from the canonical document revision;
- checked save -> `didSave` only after disk save succeeds;
- document close -> `didClose` when no longer required by the editor/LSP ownership policy;
- LSP restart replay uses the latest synchronized canonical editor text;
- disk-oriented `ensure_file_open_from_disk` cannot overwrite an editor-managed document.

Full-text LSP changes are acceptable initially. Incremental range synchronization is an optimization, not a closure requirement.

### 8. Ordinary agent tools remain disk-authoritative initially

M005 does not redefine `read`, `edit`, `write`, `replace`, `apply_patch`, Git, or shell tools to operate on unsaved buffers.

This avoids making agent tool semantics depend on a frontend-open document.

A later TUI IDE milestone may add explicit workflows such as:

- save-before-agent-run;
- apply reviewed agent patch into an open buffer;
- compare disk vs unsaved editor;
- refresh/merge conflict UI.

Those workflows consume the M005 foundation and must not be hidden mutation redirects.

### 9. TUI is the first qualification frontend

M005 closes with a minimal TUI integration proof, not a full IDE shell.

That proof exercises:

- open;
- local edit through the reusable replica/controller;
- daemon revision acceptance;
- LSP sees unsaved text;
- checked save;
- external-disk conflict;
- reload/resync;
- close.

It does not require syntax highlighting, file explorer, editor panes, terminal composition, or graphical editor work.

### 10. GUI editor work is long-term

The current desktop control-plane app does not need editor commands to close M005.

A future GUI may consume the same document service and client replica through a narrow Tauri bridge. Monaco is presentation technology, not an M005 dependency or protocol authority.

## Consequences

### Positive

- TUI IDE work can start on a durable shared editor substrate.
- A future GUI can reuse the same daemon/client/document semantics.
- LSP sees unsaved text without becoming buffer authority.
- Disk conflicts fail closed instead of silently overwriting user edits.
- Agent tools keep their existing durable filesystem/checkpoint semantics.
- Editor correctness is testable without Ratatui or WebView dependencies.
- The protocol has deterministic stale-revision behavior without CRDT complexity.

### Negative

- CodeGG gains a new in-memory daemon service and client replica lifecycle.
- Frontend optimistic editing requires explicit resync behavior.
- LSP's existing disk-oriented helper paths must distinguish editor-managed documents.
- Daemon restart loses unsaved text until a later recovery design is added.
- External mutation of dirty open files becomes an explicit conflict users must resolve.

### Neutral or deferred

- Rope selection is implementation-qualified; the ADR does not expose the rope type publicly.
- Undo/redo stacks are frontend-owned.
- Syntax highlighting/tree-sitter is deferred to the TUI IDE milestone.
- Filesystem watcher use is optional optimization; it is not correctness authority.
- CRDT/OT, collaborative live editing, durable unsaved recovery, GUI IDE, and agent-on-buffer semantics are deferred.

## Compatibility and migration

The initial document protocol is additive. Existing TUI/session/projection/file/LSP requests remain valid.

No existing on-disk storage migration is required for the M005 foundation because unsaved documents are ephemeral.

The LSP public API may gain editor-managed synchronization methods, but existing disk-oriented calls remain supported. Their behavior must fail closed or route through the document owner when an editor-managed document is active; they must never replace unsaved text with disk content.

Root CodeGG MSRV remains Rust 1.89. Any selected rope dependency must support that floor.

## Security and reliability implications

- Open/read requires canonical project/workspace scope plus `file.read`.
- Change/close of an attached document requires authenticated attachment ownership and project/session visibility as defined by the protocol plan.
- Save/reload mutations use canonical daemon scope and `file.modify`; renderer/client payloads never supply trusted absolute paths.
- Text, edits, and snapshots are bounded before allocation/application.
- Binary/non-UTF-8 files fail with a typed unsupported response in M005.
- Save reuses workspace mutation serialization and safe atomic-write/path-validation primitives; no second write authority is created.
- Stale client revisions fail explicitly and never overwrite newer daemon text.
- Disk conflicts retain unsaved text and perform zero write.
- Disconnect drops client attachment/replica ownership; it does not imply save or discard of canonical documents unless the documented document-lifetime policy says the last attachment closes an ephemeral clean document.
- Daemon restart invalidates document handles/revisions; clients reopen from disk.

## Verification

Conforming implementations must prove:

- Unicode-safe byte transactions and deterministic snapshots;
- stale/duplicate/out-of-order revision handling;
- explicit project/workspace/path binding;
- file-read/file-modify authorization;
- disk hash conflict with zero mutation;
- workspace lock reuse and no independent filesystem lock authority;
- LSP sees unsaved editor text;
- LSP restart replay keeps the latest synchronized editor text;
- disk-oriented LSP operations cannot overwrite an editor-managed dirty document;
- client optimistic replica converges or resyncs after rejection/reconnect;
- TUI and a headless client can use the same document protocol/controller without TUI state in the shared crates;
- no Node/Tauri/Monaco dependency enters root document/client crates;
- no CRDT or durable unsaved-buffer store is introduced.

## Supersession

ADR-0010 §8 is satisfied/refined by this decision. ADR-0010 remains accepted and is not superseded.
