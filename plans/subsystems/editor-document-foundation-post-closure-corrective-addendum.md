# Shared Editor Document Foundation — Post-Closure Corrective Addendum

Status: active

Repository baseline reviewed: `0dda01f362480f2b25162b0c912522bce827d048`

Predecessor roadmap:

- `plans/subsystems/editor-document-foundation-roadmap.md`

Accepted architecture:

- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`

Historical closure evidence:

- `plans/closure/editor-document-foundation/001-status.md`
- `plans/closure/editor-document-foundation/002-status.md`
- `plans/closure/editor-document-foundation/003-status.md`
- `plans/closure/editor-document-foundation/004-status.md`

Current hosted baseline:

- root `CI` run `37170182694` — SUCCESS on `0dda01f3`
- `Desktop E2E` run `37170182670` — SUCCESS on `0dda01f3`

## 1. Purpose and corrective trigger

M005-A through M005-D landed the intended architecture:

- `codegg-document` owns frontend-neutral revisioned text transactions;
- daemon `DocumentService` owns canonical ephemeral open text and one-writer leases;
- checked disk save/conflict semantics reuse canonical workspace mutation authority;
- `egglsp` mirrors managed unsaved text without becoming editor authority;
- `codegg-client::DocumentController` owns the frontend optimistic replica;
- the TUI owns only presentation state through `TuiDocumentSession`.

A post-merge source audit of `crates/codegg-client/src/document.rs` found a narrow lifecycle/concurrency defect cluster in the optimistic controller. These defects do not invalidate the daemon, protocol, text-core, or LSP ownership model, but they are unsafe to carry into a real keystroke-driven TUI editor.

The historical M005 closure records remain immutable. C001 is the current strict-closure authority for the client-controller portion of M005-D.

## 2. Confirmed findings

### Finding A — save can report clean while a newer local edit exists

`save()` currently:

1. calls `flush()`;
2. releases that operation lock;
3. reacquires the async operation lock;
4. sends `DocumentSave`;
5. unconditionally sets local `dirty = false` and `DocumentState::Synced` on `DocumentSaved`.

`apply_local()` does not participate in the async operation lock. A local edit may therefore be accepted after the save's flushed revision is selected or while the save request is in flight.

That newer edit remains pending, but the save response can overwrite client bookkeeping to clean/synced. The daemon remains correct; the frontend replica state can become false.

### Finding B — close/reload can race a newly accepted local edit

`close()` and `reload_from_disk()` serialize network operations with the async operation mutex, but `apply_local()` bypasses that gate.

A local edit accepted while `DocumentClose` is in flight can be lost from the client when the ACK causes `attachment = None`.

A local edit accepted while destructive reload is in flight can similarly race installation of the reloaded snapshot.

A real editor must never acknowledge a local mutation and then silently discard it because a lifecycle request was already in progress.

### Finding C — transient replica lock contention is reported as queue exhaustion

`apply_local()` uses `tokio::sync::Mutex::try_lock()` on the attachment and maps lock contention to `DocumentControllerError::QueueFull`.

A short bookkeeping lock held by flush/resync/snapshot code is not a full transaction queue. Under real typing, this can reject valid keystrokes with the wrong failure class.

### Finding D — the debounce flag does not own the full flush task lifetime

The current worker clears `flush_scheduled` immediately before calling `flush()`.

While that flush waits on transport, later edits may spawn more flush tasks. The async operation mutex prevents concurrent daemon mutations, but queued worker tasks can accumulate behind it. This contradicts the intended “one controller-owned flush task” invariant.

### Finding E — recovery does not restore scheduler/network state consistently

Transport or unexpected-response paths disable `network_enabled`.

A later successful `resync()` does not restore network scheduling, so the controller can appear recovered while subsequent local edits no longer auto-flush.

Open/reconnect error paths can also leave the public state at `Opening`, and `DocumentState::Error` is currently defined but not used.

### Finding F — repeated open can orphan the previous daemon attachment

`DocumentController::open()` does not require the controller to be closed. A second successful open replaces the local attachment without first sending `DocumentClose` for the old document.

The transport will eventually release the stale lease/attachment on disconnect, but an IDE switching documents through one controller could leave unnecessary daemon attachments alive for the connection lifetime.

## 3. Work classification

### Invariant

Local text accepted by `apply_local()` is never silently discarded or mislabeled clean by an async controller lifecycle operation.

### Concurrency

Synchronous local mutation and asynchronous save/reload/close/reconnect/resync have explicit linearization points and deterministic conflict rules.

### Resource ownership

At most one controller-owned background flush worker is active at a time, and a controller cannot silently orphan a previous daemon attachment through repeated open.

### Recovery

Successful recovery restores both public state and network scheduling capability.

### Documentation

Current roadmap/registry state must distinguish historical M005 closure from the active M005-D post-closure corrective, and M006 must remain blocked until C001 closes.

## 4. Non-goals

- Redesigning ADR-0011.
- Changing daemon `DocumentService` canonical ownership.
- Changing `document.v1` wire semantics unless a strictly additive error/status field is required.
- Adding CRDT/OT or multiple writers.
- Persisting unsaved drafts.
- Making agent tools consume unsaved buffers.
- Implementing the TUI editor widget, syntax highlighting, explorer, or M006 UI.
- Adding GUI/Monaco/Tauri editor support.
- Replacing `crop`.
- Replacing the one-writer lease model.
- Adding a new CI lane or concurrency framework.

## 5. Corrective milestone

### C001 — DocumentController linearization, scheduler ownership, and recovery

Status: ready.

Plan:

- `plans/implementation/editor-document-foundation-post-closure-corrective/001-document-controller-linearization-scheduler-and-recovery.md`

C001 owns:

1. replica-state synchronization that cannot spuriously reject edits on lock contention;
2. save/local-edit fencing;
3. destructive lifecycle fencing for reload/close/reconnect/resync;
4. single-worker flush scheduling;
5. recovery/network-state restoration;
6. repeated-open attachment hygiene;
7. deterministic adversarial tests;
8. current-authority planning reconciliation;
9. exact-head hosted root CI + Desktop E2E confirmation.

## 6. Target controller model

The corrective should separate:

- **short synchronous replica state** — attachment, public state, local edit sequence, pending queue metadata, lifecycle phase;
- **async network operation serialization** — daemon requests that must not overlap semantically;
- **background flush ownership** — one worker at a time.

A suitable conceptual shape is:

```rust
struct ReplicaState {
    attachment: Option<Attachment>,
    state: DocumentState,
    lifecycle: ControllerLifecycle,
    local_edit_seq: u64,
}

enum ControllerLifecycle {
    Idle,
    Saving { persisted_local_seq: u64, daemon_revision: u64 },
    Reloading,
    Closing,
    Resyncing,
    Reconnecting,
}

struct DocumentController {
    replica: std::sync::Mutex<ReplicaState>,
    operation: tokio::sync::Mutex<()>,
    flush_worker_running: AtomicBool,
    network_enabled: AtomicBool,
    // existing transport + generation/fencing
}
```

Exact types may differ.

Requirements:

- `apply_local()` remains synchronous;
- it must not use a contended async mutex `try_lock` as a normal control path;
- synchronous mutex guards are held only for bounded in-memory work and never across `.await`;
- no second text buffer is introduced;
- `codegg-document` remains the only transaction engine.

A separate `std::sync::Mutex` or equivalent short critical-section primitive is preferred over adding a new dependency solely for this fix.

## 7. Save semantics

Save should remain typing-friendly.

Required sequence:

1. serialize with other daemon lifecycle operations;
2. flush the currently queued canonical changes;
3. under the synchronous replica lock capture:
   - current daemon revision;
   - current `local_edit_seq`;
   - writer/document identity;
4. mark a save fence containing the captured sequence/revision;
5. send `DocumentSave`;
6. allow later `apply_local()` calls to update the optimistic buffer and queue new changes while the save request is in flight;
7. on successful save:
   - mark the persisted daemon revision/base as saved;
   - set client clean/synced **only if** no local edit was accepted after the save fence and no pending change exists;
   - otherwise keep `DirtyLocal`, preserve the pending queue, and wake the flush worker;
8. on save conflict:
   - preserve all local text/pending changes;
   - enter `Conflict`;
9. on transport/protocol error:
   - preserve local state and expose the correct disconnected/error/resync state.

Do not block normal typing for the entire save network round trip merely to simplify bookkeeping.

## 8. Destructive lifecycle semantics

### Close

Before sending `DocumentClose`:

- atomically enter `Closing`;
- require no pending local transaction;
- reject new `apply_local()` calls with a distinct lifecycle-busy/closing error while the close is unresolved.

On ACK:

- drop the local attachment;
- enter `Closed`.

On failure/cancellation:

- restore the previous usable state;
- keep the attachment/draft;
- ensure later edits can proceed when safe.

No edit accepted before the close linearization point may be lost.

### Reload

Reload is destructive to unsaved text.

Before request:

- enter `Reloading`;
- require the existing explicit no-pending precondition;
- reject local edits until the reload either commits or fails.

On success:

- install exactly the authoritative reloaded snapshot;
- restore `Synced`.

On failure:

- retain the original replica and restore its prior dirty/conflict state.

### Resync

Explicit resync must have defined edit behavior.

Preferred:

- enter a short `Resyncing` phase and reject new edits until the authoritative snapshot is reconciled;
- preserve already queued local draft through existing accepted-prefix/base matching logic;
- on success restore `network_enabled = true` when the transport is usable;
- if pending work remains and the writer lease is valid, wake exactly one flush worker.

### Reconnect

Offline drafting remains allowed while disconnected.

Once reconciliation begins:

- enter `Reconnecting`;
- freeze new `apply_local()` acceptance for the bounded reopen/snapshot comparison;
- reacquire writer as today;
- fence all completions by controller/document/lifecycle generation;
- restore network scheduling only after a consistent attachment is installed;
- retain divergent local drafts as `ResyncRequired`/`GoneWithLocalDraft`, never overwrite.

## 9. Open/reopen semantics

Choose one explicit controller rule and encode it in tests.

Preferred foundation rule:

- `open()` is legal only from `Closed`/no attachment;
- if already attached, return a typed `AlreadyOpen`/lifecycle error;
- switching files requires explicit `close()` then `open()`, or a future higher-level controller pool.

This avoids hidden remote attachment leakage and keeps one controller = one document identity.

Open failure state must be deterministic:

- transport failure: `Disconnected` or a documented retryable closed state;
- protocol/validation failure: `Error` or `Closed` with typed error;
- never remain stuck in `Opening`.

If `DocumentState::Error` remains public, it must have real transitions/tests; otherwise remove it before it becomes compatibility debt.

## 10. Single flush-worker invariant

The scheduled/running flag must cover the entire lifetime of the background worker, not only the debounce sleep.

Required behavior:

- at most one worker task per controller is scheduled/running;
- the worker may debounce once, then drain/send the serial queue while network is enabled;
- edits arriving during a slow network request do not spawn a pile of waiting workers;
- when the worker exits, it performs a race-safe final check so a wakeup arriving at the handoff boundary is not lost;
- transport disable stops automatic retry without spinning;
- successful reconnect/resync re-enables scheduling and wakes pending work;
- no task-per-keystroke behavior.

A lazily spawned worker using the existing atomic ownership flag is acceptable if the exit/rearm race is proven. A larger persistent worker/Notify design is not required unless the narrow approach cannot be made race-safe.

## 11. Error semantics

Add narrow client errors where useful:

- `AlreadyOpen`;
- `LifecycleBusy { operation }` or equivalent;
- preserve real `QueueFull` only for the actual pending transaction count limit;
- preserve `QueueBytes` only for the actual byte limit.

Do not map mutex contention, lifecycle transition, or transport state to queue exhaustion.

Do not expose document text in error/log output.

## 12. Required deterministic tests

Use barrier/oneshot scripted transports rather than scheduler-probability loops.

### Save/edit

- hold `DocumentSave` response;
- accept a new local edit while save is in flight;
- release save;
- prove controller remains dirty, pending text remains exact, and a follow-up flush reaches daemon;
- prove a save with no later edit becomes synced/clean.

### Close/edit

- hold `DocumentClose`;
- attempt `apply_local`;
- prove the edit is rejected as lifecycle-busy, not queue-full;
- on close failure, prove the previous attachment/text/state remain usable;
- on ACK, prove no late draft is accepted/dropped.

### Reload/edit

- hold reload;
- prove local edit cannot race snapshot replacement;
- failure restores old text/state;
- success installs authoritative text.

### Resync/recovery

- force a stale/unexpected response that disables auto network work;
- successful resync restores network scheduling;
- pending edit flushes automatically afterward;
- failed resync preserves draft/state.

### Open lifecycle

- failed open never leaves `Opening`;
- repeated open on one attached controller is rejected before a second daemon attachment is created;
- explicit close then open succeeds.

### Scheduler ownership

- slow first `DocumentChange` while 25–100 local edits arrive;
- prove maximum active flush workers is one;
- prove maximum daemon change request concurrency is one;
- prove no pending edit is stranded at worker exit;
- prove bounds still fail with real `QueueFull`/`QueueBytes`.

### Lock-contention semantics

- exercise snapshot/status/flush bookkeeping while local edits arrive;
- no valid edit returns `QueueFull` merely because another short state access occurs.

### Generation fencing

- stale completion from a prior reconnect/resync lifecycle epoch cannot overwrite current state;
- old cleanup cannot disable a newer healthy network generation.

## 13. Integration/regression verification

Retain and extend:

- `codegg-client` document-controller tests;
- `tests/document_client_trajectory.rs`;
- `src/tui/document_session.rs` ownership test;
- daemon `document_service::tests`;
- managed LSP integration.

Add one TUI-kind trajectory that:

1. opens writable document;
2. types;
3. starts save with a barrier;
4. types again;
5. completes save;
6. confirms the newer edit remains dirty and then flushes;
7. closes/reopens without leaking the prior attachment.

No full editor widget is required.

## 14. Hosted verification

Required local minimum:

```bash
cargo fmt --all -- --check
cargo test -p codegg-document --locked
cargo test -p codegg-client --locked
cargo test -p codegg --test document_client_trajectory --locked
cargo test -p codegg --lib tui::document_session --locked
cargo test -p codegg document_service::tests --lib --locked
cargo test -p codegg --features lsp-test-support --test document_lsp_integration --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
git diff --check
```

Required hosted evidence on the corrected production head:

- root `CI` — SUCCESS;
- path-gated `Desktop E2E` — SUCCESS.

`crates/codegg-client/**` is already a desktop-critical shared dependency and should continue triggering Desktop E2E. Do not add a new CI lane.

## 15. Documentation reconciliation

Do not rewrite any existing M005 closure record.

Current authority should record:

- M005-A/B/C remain closed without qualification;
- M005-D has historical closure but current strict disposition is conditional on C001;
- parent M005 is therefore conditionally closed until C001 closes;
- M006 is blocked from implementation planning/handoff while C001 is open;
- graphical IDE work remains long-term/deferred;
- after C001 closes, M005 returns to strict closed and M006 becomes eligible for fresh TUI presentation audit/planning.

The historical M005-D closure contains a parent-roadmap link to the M006 anchor rather than M005. Preserve that immutable record; note the correct parent link in the C001 closure/current roadmap instead of rewriting history.

## 16. Completion definition

C001 closes only when:

1. valid local edits are never rejected because a short replica-state lock is busy.
2. save cannot mark the controller clean/synced when a newer local edit exists.
3. save-in-flight typing remains supported and deterministic.
4. close/reload cannot discard a newly accepted local edit.
5. resync/reconnect recovery restores network scheduling correctly.
6. open/reconnect failure cannot leave stale public lifecycle state.
7. one controller cannot silently orphan a prior attachment through repeated open.
8. at most one background flush worker is active per controller.
9. worker exit cannot strand a pending edit.
10. barrier-forced lifecycle/concurrency tests pass.
11. root Clippy/quick verification pass.
12. hosted root CI and Desktop E2E pass on the corrected production head.
13. current roadmap/registry state is reconciled.
14. no unresolved high/medium client-controller correctness issue remains.

## 17. Status

| Corrective | Status | Plan | Blocker |
|---|---|---|---|
| C001 DocumentController linearization, scheduler ownership, and recovery | ready | `plans/implementation/editor-document-foundation-post-closure-corrective/001-document-controller-linearization-scheduler-and-recovery.md` | No code dependency blocker. M006 remains blocked until C001 strict-closes the M005-D client foundation. |
