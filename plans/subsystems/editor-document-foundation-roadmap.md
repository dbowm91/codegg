# Shared Editor Document Foundation Roadmap

Status: active

Repository audit baseline: `43cc33f6e740de33878d78a8fa2de959692cf819`

Parent milestone:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#milestone-005--editor-documentbuffer-and-lsp-synchronization-contract`

Accepted architecture:

- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`
- `plans/adrs/ADR-0010-desktop-frontend-and-shared-client-boundary.md`

Relevant existing architecture:

- `architecture/lsp.md`
- `architecture/client.md`
- `architecture/workspace_services.md`
- `architecture/snapshot.md`
- `architecture/authorization.md`
- `architecture/tui.md`

## 1. Goal

Deliver the M005 shared editor/document substrate required by a TUI-first CodeGG IDE while keeping graphical IDE work long-term.

M005 is complete when CodeGG has a frontend-neutral text/edit core, daemon-owned ephemeral document service, checked disk save/conflict semantics, correct LSP synchronization for unsaved text, and a reusable client-side optimistic replica/controller proven through a minimal TUI integration.

The milestone does **not** build the full TUI IDE.

## 2. Why this is a separate shared roadmap

The original M005 lived under the desktop/IDE foundation because ADR-0010 identified the missing document contract while planning the desktop frontend.

The implementation should not live conceptually under Tauri:

- the TUI is the first intended IDE frontend;
- `codegg-document`, daemon document state, protocol, LSP sync, and `codegg-client` are frontend-neutral;
- the GUI should eventually consume these components rather than dictate them.

This roadmap is therefore the implementation authority for parent M005. Closing this roadmap closes M005 in the parent roadmap.

## 3. Existing reusable substrate

### LSP

`egglsp` already owns:

- language-server process/client lifecycle;
- `didOpen`, `didChange`, `didSave`, `didClose`;
- `OpenDocumentRegistry` for successful LSP synchronization and restart replay;
- negotiated UTF-8/UTF-16/UTF-32 position encodings;
- diagnostics, completion, navigation, rename/format/code-action previews;
- semantic tokens;
- temporary semantic overlays.

What is missing is editor-document ownership above this mirror.

### Workspace/file mutation

CodeGG already owns:

- explicit project/workspace identity;
- canonical workspace roots/path policy;
- `WorkspaceLockTable`;
- safe path/symlink checks;
- hash-based pre/post state;
- atomic temp-write/rename patterns;
- edit checkpoints and checked restore;
- `FileChanged` notification;
- LSP preview application under daemon authority.

M005 must reuse/extract these primitives rather than add an editor-only write path.

### Frontend client

`codegg-client` already owns:

- native local protocol connection/reconnect;
- request correlation;
- bounded typed event delivery;
- session projection driver/controller.

M005 extends this crate with a document controller/replica rather than implementing document concurrency directly in Ratatui.

## 4. Architectural invariants

1. Daemon is canonical owner of open-document text/revision.
2. Disk is durable source authority; unsaved document bodies are ephemeral in M005.
3. Frontends own cursor/selection/viewport/modal UI state.
4. Canonical edits use UTF-8 byte ranges.
5. Stale document revisions fail explicitly; no implicit merge.
6. Dirty-buffer disk conflicts preserve unsaved text and perform zero write.
7. LSP mirrors canonical document text; it never owns editor truth.
8. Agent/model file tools remain disk-authoritative in M005.
9. Existing workspace lock/path/checkpoint authority is reused for save.
10. `FileChanged` is invalidation evidence, not correctness authority.
11. No TUI or GUI widget type enters the shared document/service/protocol crates.
12. No CRDT/OT or persistent unsaved-buffer store is introduced.
13. Root MSRV remains 1.89.
14. TUI is the first real consumer; GUI editor integration is deferred.

## 5. Milestones

### M005-A — Pure text core and transaction contract

Status: ready.

Plan:

- `plans/implementation/editor-document-foundation/001-text-core-and-transaction-contract.md`

Deliver:

- `codegg-document` leaf crate;
- rope dependency qualification;
- document revisions/snapshots;
- byte-range text transactions;
- deterministic application and inverse transaction seam;
- line/position helpers;
- bounds and property tests.

No daemon/LSP/TUI work.

### M005-B — Daemon document service and native protocol

Status: blocked on M005-A.

Plan:

- `plans/implementation/editor-document-foundation/002-daemon-document-service-and-protocol.md`

Deliver:

- daemon-owned ephemeral `DocumentService`;
- explicit project/workspace/relative-path identity;
- authorization;
- open/get/change/save/reload/close protocol;
- stale revision/resync;
- checked disk-base hash state;
- bounded native events/snapshots;
- reconnect/restart semantics.

No editor widget.

### M005-C — LSP synchronization, checked save, and external conflict

Status: closed. See `plans/closure/editor-document-foundation/003-status.md`.

Plan:

- `plans/implementation/editor-document-foundation/003-lsp-sync-checked-save-and-external-conflict.md`

Deliver:

- editor-managed LSP sync path;
- protection against disk reread clobbering unsaved LSP text;
- reuse/extraction of canonical safe-write/checkpoint primitives;
- disk conflict state;
- external mutation reconciliation;
- restart replay using canonical editor text.

No agent-on-unsaved-buffer semantic change.

### M005-D — Reusable client replica and TUI-first qualification

Status: active.

Plan:

- `plans/implementation/editor-document-foundation/004-client-replica-and-tui-first-qualification.md`

Deliver:

- `codegg-client` optimistic document replica/controller;
- stale/resync/reconnect handling;
- minimal TUI adapter/diagnostic proof;
- headless + TUI use of one document service;
- end-to-end open/edit/LSP/save/conflict/reload/close trajectory.

This is M005 closure. It does not build the full IDE shell.

## 6. Dependency graph

```text
ADR-0011 accepted
      |
      v
M005-A text core
      |
      v
M005-B daemon document service + protocol
      |
      v
M005-C LSP + checked disk conflict/save
      |
      v
M005-D codegg-client replica + TUI-first qualification
      |
      v
parent M005 closed
      |
      v
TUI IDE vertical-slice planning (parent M006)
```

M002 Windows desktop evidence is independent and does not block this Linux/macOS/root-workspace foundation.

## 7. Text-core dependency posture

The public CodeGG document contract must not expose the selected rope implementation.

M005-A begins with a bounded qualification of:

- `crop` 0.4.3 — leading candidate;
- `ropey` 1.6.1 — fallback/control.

Selection criteria:

- Rust 1.89 compatibility;
- exact UTF-8 byte-boundary editing;
- cheap immutable snapshots/clones;
- line lookup and Unicode correctness;
- no unsafe/public type leakage;
- acceptable compile/dependency footprint;
- deterministic behavior on large and adversarial edit sequences.

Do not add a permanent benchmark gate. A small local qualification fixture is enough to select the dependency.

## 8. Protocol scope

The M005 document protocol is additive and versioned/namespaced.

Expected request classes:

- capabilities;
- open;
- snapshot/get;
- change;
- save;
- reload;
- close/detach.

Expected events:

- canonical revision changed;
- saved/base advanced;
- disk conflict/invalidation;
- document closed/gone.

Exact wire shapes belong to M005-B.

Full file bodies are bounded and only returned to authorized attached clients. Events should prefer revisions/state summaries; large snapshots use request/response, not broadcast.

## 9. Document lifetime

Default M005 lifetime is process-local.

A document remains alive while needed by one or more attached frontend/LSP owners according to M005-B's explicit lease/attachment policy.

Do not infer save/discard from frontend disconnect.

No autosave is implied.

Daemon restart:

- document handles/revisions become gone;
- clients reopen from disk;
- unsaved text is lost by explicit M005 limitation;
- a future recovery journal requires a separate ADR because it changes privacy/storage/lifecycle semantics.

## 10. External mutation policy

External disk mutation classes include:

- CodeGG agent tools;
- LSP reviewed preview application;
- Git/worktree operations;
- shell/terminal commands;
- non-CodeGG programs.

Correctness rule:

- compare hashes under the canonical workspace lock before save/reload decisions;
- advisory `FileChanged` may accelerate detection;
- dirty + changed disk => conflict, preserve unsaved text;
- clean + changed disk => explicit refresh/reload according to service policy;
- no automatic merge in M005.

## 11. TUI-first boundary

After M005-D, the next IDE work should be a TUI vertical slice.

That later milestone may select/evaluate a Ratatui editor widget, syntax/semantic highlighting, panes, explorer/search, diagnostics/completion UI, terminal composition, Git/worktree views, and agent-review workflows.

Those are presentation/product features, not M005 dependencies.

A TUI widget such as `tui-textarea-2` may be evaluated, but it must operate as an adapter/replica consumer rather than become canonical document authority.

## 12. Graphical IDE boundary

The current Tauri desktop app remains a control-plane/session frontend.

M005 must not add:

- Monaco;
- xterm.js;
- JS filesystem access;
- GUI file explorer;
- GUI document bridge commands solely for product UI;
- GUI editor E2E as a closure dependency.

A future graphical IDE can add a narrow Tauri bridge over the same `codegg-client` document controller.

## 13. Security and reliability

M005 must preserve:

- `file.read` for open/read;
- `file.modify` for durable save/reload mutations where applicable;
- explicit project/workspace scope;
- relative path containment and symlink checks;
- workspace mutation lock serialization;
- zero-write stale/disk-conflict behavior;
- bounded document size/edit count/insert bytes/client attachments;
- no text bodies in audit metadata/logs;
- no renderer/client-supplied absolute path authority;
- no hidden autosave on disconnect;
- no implicit agent access to unsaved text.

## 14. Verification strategy

Use deterministic local/unit/integration tests.

Required classes:

- text-core property tests and Unicode boundary fixtures;
- stale revision/concurrent client tests;
- authorization and cross-workspace isolation;
- disk-hash conflict under forced interleavings;
- workspace lock contention;
- LSP fake-server `didOpen/change/save/close` ordering;
- LSP restart replay with dirty unsaved text;
- disk-oriented source-intelligence call while editor document is dirty;
- client optimistic edit/reject/resync/reconnect;
- headless + TUI-client coexistence;
- daemon restart gone-handle behavior.

No live external language server or GUI/WebDriver CI is required for M005 closure. Existing `egglsp` production compatibility suites remain useful regression evidence.

## 15. Documentation

Implementation should create/update:

- new `architecture/document.md`;
- `architecture/lsp.md`;
- `architecture/client.md`;
- `architecture/protocol.md`;
- `architecture/core.md`;
- `architecture/tui.md`;
- `architecture/workspace_services.md` / `architecture/snapshot.md` only where safe-write ownership moves;
- parent desktop/IDE roadmap;
- registry.

## 16. Completion definition

M005 closes when:

1. a pure shared document crate provides deterministic revisioned transactions;
2. daemon `DocumentService` owns canonical ephemeral unsaved text;
3. native protocol supports bounded open/change/save/reload/close and stale-resync;
4. disk saves are hash-checked under existing workspace mutation authority;
5. dirty external-disk conflict preserves unsaved text;
6. LSP consumes current editor text and restart replay preserves that synchronized text;
7. disk-oriented LSP helpers cannot clobber an editor-managed document;
8. `codegg-client` provides a reusable optimistic document replica/controller;
9. minimal TUI integration proves the shared controller without becoming document authority;
10. agent/model file-tool semantics remain disk-authoritative;
11. root Rust 1.89/default CLI/TUI builds remain independent of GUI tooling;
12. no unresolved high/medium ownership/concurrency/security issue remains.

## 17. Status

| Milestone | Status | Plan | Hard blocker |
|---|---|---|---|
| M005-A text core and transaction contract | closed | `plans/implementation/editor-document-foundation/001-text-core-and-transaction-contract.md` | `plans/closure/editor-document-foundation/001-status.md` |
| M005-B daemon document service + protocol | closed | `plans/implementation/editor-document-foundation/002-daemon-document-service-and-protocol.md` | `plans/closure/editor-document-foundation/002-status.md` |
| M005-C LSP sync + checked save/external conflict | closed | `plans/implementation/editor-document-foundation/003-lsp-sync-checked-save-and-external-conflict.md` | `plans/closure/editor-document-foundation/003-status.md` |
| M005-D client replica + TUI-first qualification | active | `plans/implementation/editor-document-foundation/004-client-replica-and-tui-first-qualification.md` | none |
