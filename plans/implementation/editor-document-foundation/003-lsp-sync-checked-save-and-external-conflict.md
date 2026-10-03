# Shared Editor Document Foundation M005-C — LSP Synchronization, Checked Save, and External Conflict

Status: blocked on M005-B

Repository baseline: `43cc33f6e740de33878d78a8fa2de959692cf819`

Parent roadmap:

- `plans/subsystems/editor-document-foundation-roadmap.md`

Accepted decision:

- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`

Hard dependency:

- M005-B strict closure.

Primary class: correctness / integration / mutation authority

## 1. Objective

Connect daemon-owned editor documents to:

- `egglsp` synchronization;
- checked durable save;
- external disk mutation/conflict handling.

This milestone makes unsaved editor text semantically real for CodeGG language intelligence while preserving disk and existing workspace mutation authority.

## 2. Critical existing defect to avoid

`LspService::ensure_file_open_from_disk()` currently:

1. reads file content from disk;
2. checks whether the URI is open;
3. calls `update_file` with disk text when already open.

That is correct for disk-owned LSP use today.

After M005-B it is unsafe for an editor-managed dirty document because a hover/completion/diagnostics call could overwrite the LSP mirror with stale disk content.

M005-C must establish an explicit managed-document synchronization mode before any TUI IDE relies on unsaved LSP state.

## 3. Ownership model

```text
DocumentService        = canonical editor text/revision
Disk                   = durable saved source
egglsp registry        = what was synchronized to each LSP client
LSP server             = semantic mirror
FileChanged/event bus  = invalidation hint
WorkspaceLockTable     = existing mutation serialization
```

No component may reverse those arrows.

## 4. LSP managed-document mode

Extend `egglsp` with a transport-neutral distinction between:

- disk-managed open document;
- externally/editor-managed open document.

Possible representation:

```rust
enum OpenDocumentSource {
    Disk,
    Managed,
}
```

stored in `OpenDocumentSnapshot`.

Required behavior:

- `open_file_managed(path,text)` or equivalent marks Managed;
- `update_file_managed(path,text)` preserves Managed;
- `save_file` clears dirty synchronization state but does not switch source back to Disk;
- `close_file` removes it normally;
- restart replay preserves source/text/version;
- `ensure_file_open_from_disk` sees a Managed open doc and **must not replace its text from disk**.

Do not make `egglsp` depend on `DocumentService`.

## 5. Root document/LSP coordinator

Add a root orchestration layer that knows both:

- `DocumentService`;
- `LspService`.

Responsibilities:

- on canonical document open: synchronize full current text via managed didOpen;
- on accepted document change: synchronize latest full text via managed didChange;
- on checked save: emit didSave after disk commit;
- on document close/eviction: didClose according to ownership policy;
- before semantic operation on an editor-managed doc: reconcile LSP mirror against the current canonical document snapshot if necessary;
- expose degradation/retry state without rolling back accepted user text.

The coordinator may use full-text `didChange` initially.

Do not implement LSP incremental-range optimization in M005.

## 6. LSP failure semantics

DocumentService is canonical; LSP is a mirror.

If LSP synchronization fails after a document edit was accepted:

- keep the canonical document edit/revision;
- mark/document LSP sync as degraded/stale;
- return a bounded warning/status to the client when useful;
- on next semantic operation or server recovery, full-sync the current canonical text;
- never roll back document text merely because language intelligence failed.

If an LSP process restarts after missing newer editor revisions:

- its registry replay may restore the last successfully synchronized text;
- coordinator must compare/reconcile with current DocumentService state before serving editor semantic requests.

Add a revision/digest marker at the root bridge if needed; do not overload LSP's i32 document version with the CodeGG u64 document revision.

## 7. Semantic operation routing

Audit every `egglsp` operation that calls `ensure_file_open_from_disk`, including:

- diagnostics;
- hover/navigation/references;
- completion/signature help;
- code actions;
- rename;
- formatting;
- semantic tokens;
- hierarchy operations.

For editor-managed documents, root/TUI-facing operations must synchronize canonical editor text first and then issue the operation without a disk refresh.

Disk-only agent/tool LSP calls for files not editor-managed retain current behavior.

No operation may silently switch a managed document back to Disk source.

## 8. Checked save primitive

Document save must reuse existing workspace/path mutation authority.

Create/extract one narrow checked write primitive rather than copying `src/lsp/mutation.rs::write_atomic`.

Target behavior:

```text
checked_workspace_text_write(
  workspace_root,
  relative_path,
  expected_disk_hash,
  new_utf8_text,
  workspace_lock
)
  -> Written { new_hash }
   | Conflict { actual_hash }
   | InvalidPath
   | Io
```

Requirements:

- canonical containment;
- reject symlink target/unsafe parent according to existing policy;
- acquire/reuse `WorkspaceLockTable` at the correct layer;
- reread/hash current disk content under the lock;
- expected hash mismatch => zero write;
- temp file + fsync + atomic rename;
- parent sync where existing hardened primitive does so;
- deterministic new hash;
- bounded content;
- no hidden directory creation unless current editor save policy explicitly supports creating a new document.

M005 initial open targets existing UTF-8 files. New-file creation can be deferred to TUI IDE work if it complicates authority.

## 9. Reuse/migration of LSP preview apply

`src/lsp/mutation.rs::apply_preview` already owns similar:

- workspace lock;
- expected original hashes;
- checked patch application;
- atomic writes;
- post-state capture;
- rollback/checkpoint;
- FileChanged;
- LSP update.

M005-C should reuse the new low-level checked write/path primitive where doing so preserves its multi-file atomic/rollback/checkpoint semantics.

Do **not** replace the entire LSP preview mutation service with DocumentService.

Preview apply remains disk-authoritative. If it modifies an editor-open file, it becomes an external mutation event for that document.

## 10. Save transition

Writer invokes `DocumentSave { document_id, expected_revision, writer_lease }`.

Under document + workspace serialization:

1. validate current document revision/writer;
2. snapshot canonical text and recorded disk-base hash;
3. perform checked workspace write against disk-base hash;
4. on hash conflict:
   - write nothing;
   - preserve text/revision/dirty;
   - set typed disk-conflict state;
   - return `document_disk_conflict` with structural metadata only;
5. on successful write:
   - update document disk base to new hash;
   - mark clean;
   - clear resolved conflict;
   - publish normal `FileChanged`;
   - send LSP didSave after commit;
   - return saved revision/base state.

A concurrent accepted document edit must not race through save snapshot/commit. Use document-level serialization or revision revalidation so a save cannot mark a newer dirty revision clean.

## 11. External mutation detection

Correctness does not depend on a filesystem watcher.

Triggers for revalidation:

- `FileChanged` advisory event for matching workspace/path;
- before save;
- explicit reload;
- explicit document status/snapshot operation where useful;
- before a semantic operation only if cheap metadata hints suggest disk change; do not hash disk on every completion keystroke.

Optional file watching may be added later for UX latency.

### Clean document

If disk hash changes while clean:

- mark disk-invalidated;
- explicit/automatic safe refresh may reload canonical text if no edit is racing;
- advance document revision when canonical text changes;
- synchronize LSP.

Choose one predictable default and test it.

Preferred: mark invalidated + refresh at the next explicit snapshot/open focus/reload boundary, not an asynchronous silent text replacement while the user is interacting.

### Dirty document

If disk hash differs from disk base:

- set conflict;
- preserve canonical unsaved text;
- do not auto reload/merge;
- save fails until explicit reload/discard or future merge workflow.

## 12. Reload semantics

`DocumentReload` is destructive to unsaved text and therefore:

- writer lease required;
- `file.modify` required;
- expected document revision required;
- reread bounded UTF-8 disk under canonical path validation;
- replace canonical buffer;
- advance revision;
- update disk base;
- clear dirty/conflict;
- synchronize managed LSP text;
- emit a revision/state event.

No confirmation UI is part of M005-C; the protocol is explicit and the TUI IDE will decide when to ask the human.

## 13. Interaction with agent/tools

M005 does not redirect tool reads/writes to DocumentService.

If an agent tool changes a file:

- its existing disk mutation/checkpoint behavior stays authoritative;
- matching open editor document revalidates against disk;
- dirty editor => conflict;
- clean editor => invalidated/reload policy.

If an agent reads a file while the editor has unsaved text, it reads disk in M005 unless a later explicit “include unsaved editor context” feature is designed.

Document this limitation clearly.

## 14. LSP preview interaction

For a rename/format/code-action preview created while a managed document is dirty:

- the preview request must be based on current managed LSP text, not disk;
- however the existing apply path is disk-hash/checkpoint based.

This creates an important policy boundary.

M005-C must choose one safe behavior:

Preferred foundation behavior:

- preview/semantic result may be shown from unsaved text;
- applying a disk-mutating LSP preview against a dirty editor document is rejected with a typed `document_dirty` / save-required result;
- user can save first, then regenerate/apply preview.

Do not silently apply the preview to disk while the canonical buffer has divergent unsaved text.

Future TUI IDE work may add “apply preview into buffer” as an explicit editor transaction.

## 15. Tests

### LSP mirror

- managed open sends current unsaved text;
- accepted change sends didChange;
- save sends didSave only after disk commit;
- close sends didClose;
- managed dirty + `ensure_file_open_from_disk` does not overwrite;
- LSP restart replay then coordinator reconcile reaches newest canonical text;
- LSP failure leaves document edit accepted/degraded;
- subsequent reconcile repairs mirror.

### Disk conflict

Barrier-forced:

- external write before save => conflict/zero write;
- external write racing save under workspace lock => deterministic winner;
- document change racing save does not mark newer edit clean;
- reload racing change is revision-fenced;
- clean invalidation does not tear text/revision;
- dirty conflict preserves exact unsaved text.

### Preview/tool

- LSP preview on managed dirty text sees unsaved semantics;
- preview apply to dirty document fails safely or requires save per selected policy;
- disk agent mutation triggers document conflict;
- no agent tool semantics are redirected to buffer.

## 16. Security and privacy

- no file body in `FileChanged`/audit/log metadata;
- conflict responses expose relative path/revisions/hashes only according to existing local/team policy;
- file.modify enforced before save/reload;
- checked write never accepts client absolute path;
- LSP errors do not include full text;
- no watcher scans outside registered workspace.

## 17. Documentation

Update:

- `architecture/document.md`;
- `architecture/lsp.md`;
- `architecture/workspace_services.md`;
- `architecture/snapshot.md` if low-level write primitive moves;
- `architecture/tool.md` for disk-vs-buffer agent semantics;
- `architecture/protocol.md`.

## 18. Verification

Expected minimum:

```bash
cargo test -p egglsp --locked
cargo test -p codegg --test document_lsp_integration --locked
cargo test -p codegg --test lsp_composite_stdio --locked
cargo test -p codegg --test edit_checkpoint_integration --locked
cargo test -p codegg --test checked_restore_integration --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
git diff --check
```

Use fake/production-harness LSP servers already in the repo; no public-network language server CI.

## 19. Acceptance criteria

M005-C closes when:

1. managed documents cannot be clobbered by disk-oriented LSP open helpers.
2. LSP sees unsaved canonical editor text.
3. full-text change/save/close lifecycle is correct.
4. LSP failure degrades mirror, not document truth.
5. restart/reconcile reaches latest canonical text.
6. save uses existing workspace lock/path authority and expected disk hash.
7. disk conflict performs zero write and preserves unsaved text.
8. save/change race cannot incorrectly clear dirty state.
9. external tool mutation has explicit clean/dirty behavior.
10. dirty editor + LSP disk preview apply fails safely.
11. no agent tool is transparently redirected to unsaved buffer.
12. M005-D can rely on stable protocol/state semantics.

## 20. Stop conditions

Stop and register a separate design if:

- applying LSP WorkspaceEdits directly to unsaved buffers becomes required for M005 closure;
- a filesystem watcher is required for correctness rather than UX;
- save requires a second filesystem mutation authority;
- LSP synchronization requires persisting editor text in egglsp;
- agent tools must become buffer-aware now.

## 21. Closure record

Create:

- `plans/closure/editor-document-foundation/003-status.md`

Record managed LSP contract, checked-write owner, external conflict matrix, race evidence, tool/preview disposition, and M005-D readiness.
