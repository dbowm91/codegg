# Shared Editor Document Foundation Post-Closure Corrective C001 — DocumentController Linearization, Scheduler Ownership, and Recovery

Status: active

Repository baseline: `0dda01f362480f2b25162b0c912522bce827d048`

Source corrective addendum:

- `plans/subsystems/editor-document-foundation-post-closure-corrective-addendum.md`

Historical M005-D closure:

- `plans/closure/editor-document-foundation/004-status.md`

Accepted architecture:

- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`

Primary class: concurrency / lifecycle / client correctness

## 1. Objective

Repair the optimistic `codegg-client::DocumentController` lifecycle so it is safe for a real keystroke-driven TUI editor.

The corrective must:

- preserve immediate optimistic local editing;
- linearize local mutations with async save/reload/close/reconnect/resync;
- retain edits accepted while save is in flight;
- prevent destructive lifecycle operations from silently discarding accepted drafts;
- remove false queue-full errors caused by mutex contention;
- own exactly one background flush worker;
- restore network scheduling after successful recovery;
- prevent repeated-open attachment leaks;
- preserve the M005 daemon/LSP/protocol architecture unchanged.

## 2. Confirmed source defects

Baseline `crates/codegg-client/src/document.rs` has these concrete issues:

1. `save()` unconditionally sets `dirty = false` / `Synced` after `DocumentSaved`, even if `apply_local()` accepted a newer transaction during the save request.
2. `close()` can ACK and set `attachment = None` after a local edit was accepted while close was in flight.
3. destructive reload can race a local edit because `apply_local()` does not participate in the async operation gate.
4. `apply_local()` maps `attachment.try_lock()` contention to `QueueFull`.
5. `schedule_flush()` clears `flush_scheduled` before the flush begins, permitting multiple worker tasks to queue behind the operation mutex.
6. unexpected/transport responses can disable `network_enabled`; successful `resync()` does not consistently re-enable/rearm the worker.
7. `open()`/reconnect error paths can leave public state in `Opening`.
8. repeated `open()` can replace the local attachment without explicitly detaching the old daemon document.
9. `DocumentState::Error` is currently dead public state.

## 3. Invariants

- Any `Ok` from `apply_local()` means the local text is retained until an explicit user-directed destructive transition.
- No async response may make state older than a later accepted local edit.
- Save persists a particular daemon/local fence, not “whatever the client happens to contain when the response arrives.”
- Close/reload have a clear linearization point and cannot race accepted local edits across it.
- `QueueFull` and `QueueBytes` mean actual configured resource exhaustion only.
- One controller has at most one running/scheduled flush worker.
- Daemon `DocumentService` remains canonical for accepted server text/revision.
- One writer/document remains unchanged.
- LSP remains a mirror.
- Agent tools remain disk-authoritative.
- No second TUI text buffer is introduced.
- Root MSRV remains Rust 1.89.
- Existing protocol stays additive/compatible.

## 4. Work package A — Consolidate short synchronous replica state

Refactor controller bookkeeping so `apply_local()` can take a normal short synchronous lock without `try_lock`.

Preferred:

```rust
struct ReplicaState {
    attachment: Option<Attachment>,
    public_state: DocumentState,
    lifecycle: ControllerLifecycle,
    local_edit_seq: u64,
    lifecycle_epoch: u64,
}
```

held by `std::sync::Mutex<ReplicaState>` or an equivalent existing dependency.

Rules:

- never hold the synchronous guard across `.await`;
- keep rope transaction application and queue bookkeeping bounded under the guard;
- async request payloads are copied out before awaiting;
- response application reacquires the guard and verifies epoch/document identity;
- eliminate the false `try_lock -> QueueFull` path.

If keeping separate state/attachment locks is demonstrably simpler, the implementation must still provide one synchronous linearization point for `apply_local` versus lifecycle phase changes.

## 5. Work package B — Add lifecycle epoch/phase

Introduce internal lifecycle state sufficient to fence asynchronous completions:

```text
Idle
Saving
Reloading
Closing
Resyncing
Reconnecting
```

Requirements:

- increment or otherwise uniquely identify lifecycle generations where stale completions are possible;
- every async lifecycle completion verifies document identity + lifecycle epoch before applying state;
- old completion/cleanup cannot overwrite a newer open/reconnect;
- public `DocumentState` remains a frontend-facing status, not the sole synchronization primitive.

Use the existing fixed controller generation only where it genuinely identifies the controller instance. Do not describe it as a reconnect generation unless it changes per reconnect.

## 6. Work package C — Make save fence-aware while preserving typing

Save algorithm:

1. acquire async operation ownership;
2. flush the pre-save pending queue;
3. under replica lock capture:
   - document/project/lease;
   - daemon revision;
   - `saved_local_seq = local_edit_seq`;
   - lifecycle epoch;
4. enter `Saving`;
5. release replica lock and send `DocumentSave`;
6. `apply_local()` remains allowed during `Saving` and increments `local_edit_seq`;
7. apply save response only if identity/epoch still match;
8. on success:
   - if `local_edit_seq == saved_local_seq` and pending queue empty: clean + `Synced`;
   - otherwise: keep dirty + `DirtyLocal` and wake the flush worker;
9. on disk conflict: retain all local text and enter `Conflict`;
10. on transport failure: preserve draft/pending and enter `Disconnected`;
11. on protocol/stale response: preserve draft and enter `ResyncRequired`/typed error.

Add a test hook/barrier around the save request so the race is deterministic.

## 7. Work package D — Fence destructive transitions

### Close

- transition atomically to `Closing` before request;
- require pending queue empty;
- reject `apply_local()` while Closing with `LifecycleBusy` (or equivalent);
- on ACK, clear attachment/state;
- on failure/cancellation, restore the previous attachment/public state;
- no local draft accepted before the close fence may disappear.

### Reload

- transition to `Reloading`;
- reject local edits during the destructive request/snapshot installation;
- preserve pre-reload replica on failure;
- install authoritative snapshot atomically on success.

### Resync

- transition to `Resyncing`;
- reject new edits for the bounded reconciliation window;
- preserve existing queued draft via accepted-prefix/base matching;
- successful resync restores `network_enabled`;
- pending writable work wakes the flush worker;
- failure leaves draft intact with explicit state.

### Reconnect

- disconnected mode may continue accepting bounded local drafts as already intended;
- once reconnect reconciliation begins, freeze new local edits until the authoritative snapshot/lease decision is complete;
- use lifecycle epoch fencing;
- successful reconnect enables network and wakes pending work exactly once;
- divergent/missing server state retains local draft.

## 8. Work package E — Open/attachment hygiene

Make one controller represent one attached document at a time.

Preferred behavior:

- `open()` requires no existing attachment;
- second open returns `DocumentControllerError::AlreadyOpen`;
- file switching is explicit close/create/open at the higher layer.

Also:

- transport failure during open cannot leave `Opening`;
- protocol error during open cannot leave `Opening`;
- writer-busy fallback-to-read-only behavior, if retained, is documented and tested rather than swallowing arbitrary writer-acquire errors;
- remove `DocumentState::Error` if unnecessary, or wire explicit transitions to it.

Do not auto-close the previous document inside `open()` unless that behavior is separately justified; implicit remote side effects make failure recovery harder.

## 9. Work package F — Own one flush worker

Replace the current “debounce scheduled” interpretation with “worker scheduled/running.”

Minimum acceptable algorithm:

- CAS false -> true before spawning;
- leave flag true through debounce **and** flush/drain work;
- worker sends serially under the existing async operation owner;
- worker stops when:
  - queue empty;
  - network disabled;
  - controller no longer writable/open;
  - lifecycle temporarily disallows flushing;
- before exit, perform a race-safe queue/wakeup check;
- only after worker ownership is relinquished may another worker be spawned.

If needed, use a small helper such as:

```text
run_flush_worker()
needs_flush_after_release()
```

to make the handoff testable.

Do not introduce one task per edit. Do not add a persistent runtime/thread.

## 10. Work package G — Recovery state restoration

Normalize request failure handling.

At minimum:

- transport failure => `Disconnected`, network disabled, draft retained;
- stale/protocol divergence => `ResyncRequired`, draft retained;
- conflict => `Conflict`;
- successful explicit resync/reconnect => network enabled;
- if writable pending queue remains after recovery => schedule exactly one worker;
- failed open => non-Opening state;
- failed close/reload => pre-operation usable state;
- no dead public state variants.

Audit `flush()`, `save()`, `reload_from_disk()`, `resync()`, `reconnect()`, `open()`, and `close()` together so one method does not reintroduce inconsistent transitions.

## 11. Work package H — Adversarial test matrix

Implement deterministic barrier transports in `codegg-client` tests.

Required tests:

1. **save + edit**
   - block save response;
   - accept local edit;
   - release response;
   - state remains dirty;
   - new transaction survives and flushes.

2. **save without later edit**
   - becomes clean/synced.

3. **close + edit**
   - edit during Closing fails with lifecycle-busy, not QueueFull;
   - failed close restores usable attachment;
   - successful close cannot drop an accepted post-fence edit because none can be accepted.

4. **reload + edit**
   - lifecycle-busy during reload;
   - failure preserves old snapshot;
   - success replaces exactly once.

5. **resync rearm**
   - force network-disabled/resync-required;
   - successful resync enables network;
   - pending work auto-flushes.

6. **reconnect generation**
   - stale response/cleanup from old epoch cannot change current replica.

7. **open failure**
   - no permanent `Opening`.

8. **repeated open**
   - fails before issuing a second DocumentOpen/attachment mutation.

9. **slow-flush burst**
   - 25–100 local edits during one blocked request;
   - max flush worker count = 1;
   - max DocumentChange request concurrency = 1;
   - all accepted edits eventually converge.

10. **real queue bounds**
    - QueueFull only at transaction-count limit;
    - QueueBytes only at byte limit.

11. **state-access contention**
    - concurrent snapshot/status bookkeeping does not produce false QueueFull.

Use explicit counters/barriers, not sleeps as the correctness oracle.

## 12. Work package I — TUI/integration qualification

Extend `tests/document_client_trajectory.rs` and/or the existing TUI seam tests.

Required trajectory:

1. open writable file through TUI-kind shared controller;
2. local edit + flush;
3. start save and pause response;
4. local edit during save;
5. complete save;
6. verify local state remains dirty;
7. flush newer edit;
8. verify daemon snapshot/revision;
9. explicit close;
10. reopen same/another file without stale attachment/lease;
11. LSP managed text remains current.

Keep this presentation-neutral. Do not add the actual editor widget.

## 13. Work package J — Planning/documentation reconciliation

Preserve immutable historical closure files.

Update current authority:

- `plans/subsystems/editor-document-foundation-post-closure-corrective-addendum.md` → closed on completion;
- create `plans/closure/editor-document-foundation-post-closure-corrective/001-status.md`;
- `plans/subsystems/editor-document-foundation-roadmap.md`:
  - M005-A/B/C historical/current closed;
  - M005-D historical closure + C001 current strict authority;
  - parent M005 conditional while C001 open, strict closed after;
- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md`:
  - M005 conditional while C001 open;
  - M006 blocked while C001 open;
  - after C001, M006 returns to eligible-for-fresh-planning;
- `plans/registry.md`:
  - active corrective row;
  - C001 dependency-ready row;
  - remove M006 from eligible-for-planning while open;
  - add M006 blocked row;
  - reconcile desktop/editor gates.

Historical `plans/closure/editor-document-foundation/004-status.md` has a parent-roadmap link pointing to the M006 anchor. Do not rewrite it; record the correct M005 parent in the C001 closure/current authority.

## 14. Required verification

Focused:

```bash
cargo test -p codegg-document --locked
cargo test -p codegg-client --locked
cargo test -p codegg --test document_client_trajectory --locked
cargo test -p codegg --lib tui::document_session --locked
cargo test -p codegg document_service::tests --lib --locked
cargo test -p codegg --features lsp-test-support --test document_lsp_integration --locked
```

Broad:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
git diff --check
```

Hosted on the corrected production head:

- root `CI` — SUCCESS;
- `Desktop E2E` — SUCCESS.

Do not weaken or bypass existing CI.

## 15. Acceptance criteria

C001 is complete when:

1. `apply_local()` has a real synchronous linearization point and no false try-lock QueueFull path.
2. save-in-flight edits are retained and leave the controller dirty until subsequently flushed/saved.
3. close/reload cannot race and discard accepted text.
4. resync/reconnect restore network scheduling and wake pending work.
5. repeated open cannot orphan an attachment.
6. open/reconnect/close/reload failures leave deterministic public state.
7. one flush worker owns scheduling at all times.
8. no worker-exit wakeup is lost.
9. barrier-forced concurrency tests pass.
10. TUI-kind trajectory passes without adding editor UI.
11. daemon/LSP/document-core semantics remain unchanged except any narrow compatibility-safe client fixes.
12. root Clippy/quick verification pass.
13. hosted root CI and Desktop E2E pass on the corrected head.
14. current planning state is reconciled.
15. M006 is re-released to fresh planning only after C001 closure.
16. no unresolved high/medium controller correctness finding remains.

## 16. Stop conditions

Stop and register a separate design/corrective if:

- fixing the client requires changing one-writer daemon semantics;
- protocol needs a breaking change rather than an additive client-visible status;
- save correctness requires blocking all typing for network duration;
- multiple simultaneous editor writers become required;
- a full editor widget/UI is needed to prove the fix;
- a generic async actor/runtime is proposed solely for this one controller.

## 17. Closure record

Create:

- `plans/closure/editor-document-foundation-post-closure-corrective/001-status.md`

Record:

- implementation commit(s);
- old/new controller synchronization model;
- save fence semantics;
- destructive lifecycle fence semantics;
- worker ownership algorithm;
- recovery-state matrix;
- barrier test evidence;
- TUI-kind integration trajectory;
- local broad verification;
- hosted root CI run;
- hosted Desktop E2E run;
- planning reconciliation;
- final M005/M006 disposition.

## 18. Handoff note

This is a prerequisite hardening pass for M006, not M006 itself.

Do not use C001 to add editor widgets, syntax highlighting, file explorer, keymaps, terminal panels, GUI bridge methods, or agent-on-unsaved-buffer behavior. Once C001 closes, perform the fresh TUI IDE presentation audit before writing M006 implementation plans.
