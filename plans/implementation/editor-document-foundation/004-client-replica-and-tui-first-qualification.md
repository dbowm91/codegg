# Shared Editor Document Foundation M005-D — Client Replica and TUI-First Qualification

Status: blocked on M005-C

Repository baseline: `43cc33f6e740de33878d78a8fa2de959692cf819`

Parent roadmap:

- `plans/subsystems/editor-document-foundation-roadmap.md`

Accepted decision:

- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`

Hard dependency:

- M005-C strict closure.

Primary class: reusable client / frontend foundation / qualification

## 1. Objective

Add a reusable optimistic document replica/controller to `codegg-client` and prove the complete M005 contract through a TUI-first, frontend-neutral trajectory.

This is the M005 closure milestone.

It must not become a full TUI editor implementation.

## 2. Why a client replica is required

A frontend cannot wait for a daemon round trip before rendering every typed character.

At the same time, letting the TUI own unrelated text truth would defeat ADR-0011.

The reusable client controller therefore owns:

- optimistic local replica;
- pending transaction queue;
- daemon revision tracking;
- stable change IDs;
- serial flush;
- stale/reconnect resync;
- writer lease/document attachment lifecycle.

It does not own cursor, selection, viewport, keymaps, or rendering.

## 3. Target structure

Suggested `codegg-client` modules:

```text
document/
  controller.rs
  replica.rs
  state.rs
```

Target state:

```text
DocumentController
  LocalSocketClient / Core request client
  connection_generation
  document_id
  attachment/writer lease
  DocumentReplica
  one in-flight change
  bounded pending local transaction queue
  flush task ownership/cancellation
  status stream
```

`DocumentReplica` uses `codegg-document`; it does not duplicate rope/edit logic.

## 4. Replica state machine

At minimum:

```text
Closed
Opening
Synced
DirtyLocal
Flushing
ResyncRequired
Conflict
ReadOnly
Disconnected
Gone
Error
```

Exact enum may combine states but transitions must be explicit/tested.

Distinguish:

- document dirty relative to disk;
- local pending edits not yet acknowledged by daemon;
- daemon disk-conflict state;
- transport disconnected.

Do not collapse all into one `dirty` boolean.

## 5. Optimistic edit pipeline

Frontend calls a local API such as:

```rust
controller.apply_local(transaction)
```

Behavior:

1. validate/apply immediately against replica using `codegg-document`;
2. update local snapshot synchronously/cheaply;
3. assign stable `change_id`;
4. enqueue bounded pending change;
5. background flush serializes changes to daemon.

Only one daemon change is in flight/document initially.

While change N is in flight, local change N+1 may apply against the optimistic text and queue behind it.

Because the daemon is single-writer and accepts N exactly, N+1 ranges remain valid against the daemon state after N commits.

No general operational transform is required.

## 6. Coalescing

Typing can generate many tiny transactions.

Implement bounded coalescing before network flush when safe:

- short debounce window or event-loop tick aggregation;
- preserve deterministic order;
- do not merge across explicit save/reload/undo boundary;
- cap edit count/insert bytes;
- if coalescing would require coordinate rebasing that is not trivially correct, flush separate transactions.

Correctness beats keystroke compression.

Record basic request-rate evidence but do not create a latency benchmark gate.

## 7. Idempotency and uncertain commit

The daemon M005-B change-id dedupe is mandatory for reconnect correctness.

If transport drops after sending a change but before response:

- keep the same `change_id`;
- reconnect/reacquire attachment/writer as allowed;
- retry identical request or snapshot first according to controller policy;
- duplicate accepted request converges to the original result.

If server snapshot text/revision proves the optimistic change is already canonical, mark it acknowledged.

If server state diverged unexpectedly:

- do not silently replay arbitrary local pending edits;
- enter `ResyncRequired`;
- retain the local draft snapshot/pending transactions in memory for the frontend to offer recovery;
- fetch authoritative snapshot.

Since M005 has one writer, divergence should be exceptional and is a correctness signal.

## 8. Stale response/generation fencing

Every async completion is fenced by:

- client connection generation;
- controller instance generation;
- document id;
- writer lease id where relevant;
- change id.

A completion from an old connection/document must never mutate the current replica.

Follow the lifecycle patterns already proven in desktop C001/C002 and session projection driver.

## 9. Reconnect behavior

On transport loss:

- keep local replica/draft in memory;
- mark disconnected;
- stop new network flush but permit bounded local edits only if product policy accepts offline drafting.

Preferred M005 behavior: allow local drafting while disconnected up to normal document limits, but do not claim it is daemon accepted.

On reconnect:

1. reopen/reattach same scoped document;
2. reacquire writer if available;
3. fetch authoritative snapshot;
4. resolve known in-flight `change_id`;
5. if authoritative state matches acknowledged prefix, continue pending queue;
6. otherwise enter resync/recovery state rather than automatic merge.

Test daemon restart separately: old document id/revision is gone; local unsaved draft can be retained client-side for comparison/recovery, but must not auto overwrite newly opened disk state.

## 10. Save/reload APIs

Controller exposes narrow operations:

- `save()`;
- `reload_from_disk()`;
- `snapshot()`;
- `close()`.

Save first flushes/awaits pending canonical changes, then sends `DocumentSave` for the acknowledged revision.

If disk conflict:

- keep replica text;
- expose typed conflict;
- do not clear local dirty/draft state.

Reload:

- must have no unresolved pending flush or explicitly cancel/discard it;
- invokes daemon reload with current expected revision/writer lease;
- installs returned canonical snapshot;
- frontend decides whether to prompt before invoking.

## 11. TUI-first integration boundary

Do not select/build the full TUI editor widget in M005-D.

Add the smallest reusable TUI ownership seam required for M006, for example:

- an `AppServices`/frontend controller handle that can open a document controller for the active project/workspace;
- TUI route generation fencing around async open;
- a presentation-neutral `TuiDocumentSession` adapter if necessary.

The TUI-specific layer may own:

- active document id;
- cursor placeholder/selection state for tests;
- user-facing status conversion.

It must not own a second text buffer.

No source file tree, pane layout, syntax highlighting, modal editor, or file-save keybindings are required.

## 12. TUI/headless qualification trajectory

Use a deterministic workspace and fake LSP server.

Trajectory:

1. start daemon with explicit project/workspace;
2. connect TUI-kind client using shared `codegg-client`;
3. open a UTF-8 source file writable;
4. install controller snapshot;
5. apply local transaction and assert immediate replica text;
6. flush and assert daemon revision;
7. query LSP diagnostic/semantic fixture proving unsaved text was synchronized;
8. save and assert disk hash/content + clean state;
9. make external disk mutation through a separate authorized fixture/tool path;
10. edit document dirty;
11. save => typed conflict/zero overwrite;
12. reload explicitly;
13. assert replica + daemon + LSP converge;
14. disconnect/reconnect around an in-flight idempotent change and prove no duplicate;
15. close/detach;
16. open the same file through a headless/second read client and prove protocol parity.

This proves the foundation, not IDE UX.

## 13. Two-client tests

Required:

- one writer + one read-only observer;
- observer polls authorized `DocumentStatusGet`, receives revision metadata, and resnapshots;
- observer cannot change/save/reload;
- writer disconnect releases lease;
- authorized second client can acquire writer after cleanup and sees preserved dirty canonical text;
- stale old writer completion cannot mutate new controller;
- closing read observer does not close canonical dirty document.

No simultaneous two-writer merge.

## 14. Local draft recovery semantics

Client-side unsent text is ephemeral and process-local.

On daemon restart/gone document:

- controller exposes `GoneWithLocalDraft` or equivalent when local text differs from newly opened disk;
- no automatic overwrite;
- frontend can later offer copy/diff/reapply.

M005 does not persist this draft to disk.

## 15. Performance/footprint expectations

Collect lightweight evidence:

- opening/editing ~1 MiB source file does not clone the whole buffer per keystroke in the normal replica path;
- snapshots/clones use the selected rope's sharing;
- event/request queues are bounded;
- one controller/document does not spawn an unbounded task per edit.

No permanent benchmark threshold.

## 16. Documentation

Update:

- `architecture/document.md`;
- `architecture/client.md`;
- `architecture/tui.md`;
- `architecture/protocol.md`;
- parent `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md`;
- `plans/registry.md`.

Document explicit M005 limitations:

- no persistent unsaved recovery;
- one writer/document;
- no agent reads from unsaved buffer;
- no CRDT;
- no GUI editor;
- no LSP preview apply into dirty buffer.

## 17. Verification

Expected minimum:

```bash
cargo test -p codegg-document --locked
cargo test -p codegg-client --locked
cargo test -p codegg --test document_service_integration --locked
cargo test -p codegg --test document_lsp_integration --locked
cargo test -p codegg --test document_client_trajectory --locked
cargo test -p codegg --lib tui:: --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
git diff --check
```

Use existing fake LSP/test runtime; no GUI WebDriver or live external server.

## 18. Acceptance criteria

M005-D and parent M005 close when:

1. codegg-client has a reusable document replica/controller.
2. local edits render/apply optimistically without duplicating text-core logic.
3. daemon flush is serialized/bounded/idempotent.
4. uncertain commit/reconnect does not duplicate text changes.
5. stale generations cannot mutate current replica.
6. save waits for canonical pending changes.
7. disk conflict retains unsaved replica.
8. daemon restart/gone state does not auto overwrite local draft.
9. one writer + observers behave correctly.
10. TUI-kind client uses the shared controller without owning a second buffer.
11. unsaved text is visible to LSP before save.
12. full deterministic open/edit/LSP/save/conflict/reload/reconnect/close trajectory passes.
13. no GUI/Tauri/Monaco dependency or editor UI was required.
14. no unresolved high/medium M005 finding remains.

## 19. Roadmap transition

On closure:

- create `plans/closure/editor-document-foundation/004-status.md`;
- mark M005-A through D closed;
- mark parent desktop/IDE M005 closed;
- change parent M006 to **eligible for fresh TUI IDE vertical-slice planning**;
- keep graphical IDE integration long-term/deferred;
- keep M002 Windows evidence independent.

Do not write the M006 implementation plan as part of M005 closure unless separately requested.

## 20. Stop conditions

Stop and re-plan if:

- optimistic replica correctness requires CRDT/OT with the one-writer model;
- daemon protocol cannot provide stable idempotency for uncertain commits;
- the TUI requires a second independent mutable buffer to integrate;
- GUI editor implementation becomes necessary for closure;
- agent tool semantics must change to consume unsaved buffers.

## 21. Closure record

Create:

- `plans/closure/editor-document-foundation/004-status.md`

Record client state machine, idempotency/reconnect evidence, two-client trajectory, TUI ownership proof, LSP/disk conflict trajectory, footprint evidence, limitations, and M006 planning disposition.
