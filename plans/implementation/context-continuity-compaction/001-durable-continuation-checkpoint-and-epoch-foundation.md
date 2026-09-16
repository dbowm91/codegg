# Context Continuity and Compaction M001 — Durable Continuation Checkpoint and Epoch Foundation

Status: implemented

Repository baseline: `db3b69d94fa790bf59a9b0ac093572754eb133af`

Source subsystem roadmap:

- `plans/subsystems/context-continuity-compaction-roadmap.md`

Long-term requirements:

- `plans/000-long-term-specification.md#4.2-explicit-ownership`
- `plans/000-long-term-specification.md#4.6-progressive-disclosure`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#28-observability`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/003-planning-process.md`

Applicable ADRs:

- None required at handoff. Stop and register an ADR if durable continuation authority would leave `codegg-core` session storage or require a public cross-service storage contract.

Primary class: invariant / infrastructure

## 1. Objective

Create the durable host-owned foundation required for safe multi-window continuation before changing compaction behavior.

M001 introduces:

- a typed continuation-checkpoint model with explicit lineage and lifecycle;
- a bounded append-oriented SQLite store for checkpoint candidates;
- restart-safe `Prepared | Installed | Aborted` state;
- atomic installation of a checkpoint and its durable `ContextCompacted` event;
- typed load/query APIs used by later milestones;
- compatibility-safe optional lineage fields on `ContextCompactedEvent`;
- storage/documentation updates.

M001 does **not** change which messages the compactor retains, what the model sees after compaction, or which compaction strategy is selected. It establishes persistence and state-transition semantics first.

## 2. Why this milestone is ready

The prerequisite ownership work is already closed:

- `src/context/compaction.rs` is the canonical production compaction owner.
- session and event persistence already live in `codegg-core`.
- SQLite migration policy is established and restart-safe.
- the current storage layout is version 56 at the reviewed baseline.
- `ContextCompactedEvent` already exists and can be extended additively.
- production artifacts already use a restart-durable `FileArtifactStore`.

No unresolved provider, scheduler, Git, authorization, or external-package dependency blocks a bounded additive persistence milestone.

## 3. Current implementation evidence

Implementation must re-inspect at least:

- `crates/codegg-core/src/session/schema.rs`
- `crates/codegg-core/src/storage/mod.rs`
- `crates/codegg-core/src/session/events.rs`
- `crates/codegg-core/src/session/store.rs`
- `crates/codegg-core/src/session/mod.rs`
- `src/context/compaction.rs`
- `src/agent/context_runtime.rs`
- `architecture/session.md`
- `architecture/compaction.md`
- `architecture/context-compaction-ownership.md`

Baseline findings to preserve in the implementation record:

1. `STORAGE_LAYOUT_VERSION` is 56 and `migrate_v56` is the highest wired migration. If another migration lands first, use the next free sequential version rather than forcing v57.
2. `ContextCompactedEvent` currently stores message counts, token estimates, and pinned/summarized/dropped item lists, but production compaction does not append this durable event.
3. `AgentLoop::compact_if_needed()` publishes only the in-process `AppEvent::CompactionTriggered`.
4. `EventStore::append_idempotent()` already provides collision/idempotency semantics and demonstrates restart-safe event handling.
5. the historical `checkpoints` table serializes a different `Checkpoint` type and is not a compatible continuation-state store.
6. `.codegg/goals/*.checkpoint.md` is an append-oriented goal journal and must remain separate from internal continuation checkpoint storage.

## 4. Invariants

M001 MUST preserve:

- `codegg-core` as durable session-state owner.
- `src/context/compaction.rs` as compaction-policy owner.
- no provider/model call from the checkpoint store.
- no hidden-reasoning persistence.
- no checkpoint body in logs or ordinary event payloads.
- old `ContextCompactedEvent` JSON remains deserializable.
- old databases migrate forward without rewriting historical transcript or goal data.
- checkpoint installation is not inferred from presence of a prepared row.
- `latest_installed(session_id)` never returns a `Prepared` or `Aborted` row.
- a stale checkpoint candidate cannot install over a newer installed parent.
- restart can distinguish an abandoned preparation from a committed context epoch.
- all payloads and diagnostic strings are bounded before SQLite insertion.

## 5. Scope

### In scope

- continuation checkpoint domain types;
- SQLite table/index/migration;
- `ContinuationCheckpointStore`;
- prepared/installed/aborted lifecycle;
- lineage and digest verification;
- atomic install + durable `ContextCompacted` event write;
- focused migrations/restart/contention tests;
- architecture docs and exports.

### Out of scope

- assembling goal/todo/user intent into the payload;
- rendering checkpoint text to the model;
- new `ctx://` handle kinds;
- changing compaction strategy/defaults;
- loading continuation state into `TurnRuntime`;
- multi-compaction trajectory tests beyond the store/state machine;
- checkpoint garbage collection beyond bounded query/delete helpers needed by tests.

## 6. Required production changes

### 6.1 Core/domain types

Add a small module under the existing session/core ownership, for example:

```text
crates/codegg-core/src/session/continuation.rs
```

Use final naming consistent with current module organization.

Define typed equivalents of:

```rust
pub enum ContinuationCheckpointStatus {
    Prepared,
    Installed,
    Aborted,
}

pub struct ContinuationCheckpoint {
    pub id: String,
    pub session_id: String,
    pub sequence: i64,
    pub previous_installed_id: Option<String>,
    pub schema_version: u32,
    pub status: ContinuationCheckpointStatus,
    pub payload_digest: String,
    pub payload: ContinuationCheckpointPayload,
    pub created_at: i64,
    pub installed_at: Option<i64>,
    pub aborted_at: Option<i64>,
    pub abort_reason: Option<String>,
}
```

The payload type in M001 should establish a versioned envelope and bounded serialization contract without prematurely owning all M002 semantic fields. It may contain a minimal `serde_json::Value`/versioned body internally if necessary, but callers must not bypass the typed constructor/size/digest checks.

Prefer an explicit checkpoint schema version independent of SQLite storage version. SQLite migration version answers “can this database represent checkpoints?”; checkpoint schema version answers “can this runtime decode this checkpoint payload?”

Use UUID identity consistent with repository conventions. Do not use a filesystem path as identity.

### 6.2 Storage and migration

Add a dedicated table rather than overloading the historical `checkpoints` table.

At the reviewed baseline, the intended next migration is v57:

```text
continuation_checkpoint
  id                         TEXT PRIMARY KEY
  session_id                 TEXT NOT NULL
  sequence                   INTEGER NOT NULL
  previous_installed_id      TEXT NULL
  schema_version             INTEGER NOT NULL
  status                     TEXT NOT NULL
  payload_digest             TEXT NOT NULL
  payload_json               TEXT NOT NULL
  abort_reason               TEXT NULL
  created_at                 INTEGER NOT NULL
  installed_at               INTEGER NULL
  aborted_at                 INTEGER NULL
```

Add indexes sufficient for:

- latest installed checkpoint by session;
- checkpoint lookup by session/id;
- deterministic lineage/debug inspection.

A uniqueness constraint should prevent duplicate sequence allocation within one session. Sequence gaps from abandoned candidates are acceptable.

The store must enforce a bounded serialized payload before insert. Use a conservative constant (for example 128 KiB) and document it; do not make it user-configurable in M001.

Bump `storage::STORAGE_LAYOUT_VERSION` with the migration. Update the existing project-catalog/storage invariant checks only as required by the repository's current dynamic version assertion; do not add a second version guard.

### 6.3 Store API and transaction semantics

Provide APIs equivalent to:

```text
prepare(session_id, previous_installed_id, payload)
get(session_id, checkpoint_id)
latest_installed(session_id)
mark_aborted(session_id, checkpoint_id, reason)
install_with_compaction_event(session_id, checkpoint_id, event)
```

`prepare`:

1. validates session/checkpoint fields and payload bounds;
2. computes or verifies the payload digest;
3. enters `BEGIN IMMEDIATE`;
4. reads the latest installed checkpoint;
5. checks the caller's expected parent/precondition;
6. allocates the next sequence;
7. inserts a `Prepared` immutable payload;
8. commits;
9. returns the persisted row.

`install_with_compaction_event`:

1. enters one SQLite transaction;
2. reloads the checkpoint;
3. requires `Prepared`;
4. verifies its digest and expected latest-installed parent;
5. changes status to `Installed` and timestamps it;
6. inserts the matching `SessionEvent::ContextCompacted` using a stable event ID/idempotency key derived from the checkpoint identity;
7. commits both operations together.

Reuse/refactor the existing `EventStore` serialization/conflict logic through an internal transaction-aware helper (for example `append_in_tx`) rather than hand-maintaining a second event-row encoding path inside the continuation store.

A duplicate retry of the same install must converge idempotently. A conflicting payload/event for the same identity must fail closed.

`mark_aborted` must be idempotent for an already-aborted row and must refuse to turn an `Installed` checkpoint into `Aborted`.

### 6.4 ContextCompacted event

Extend `ContextCompactedEvent` additively with `#[serde(default)]` optional fields such as:

```text
checkpoint_id
checkpoint_digest
epoch_sequence
previous_checkpoint_id
continuity_degraded_reason
```

Exact names may differ. Old stored JSON and old constructors/tests must remain readable.

Do not put the continuation payload itself into `session_events`; the event is structural evidence and a commit marker.

M001 should add one small application-facing helper/service so later `AgentLoop` integration does not need to manually serialize SQL/event details.

### 6.5 Protocol / DTOs

No frontend protocol change is required in M001.

Do not expose full checkpoint payloads through `CoreEvent`, ACP, observer projections, or HTTP merely because the store now exists.

If a small path-free diagnostic field is already propagated from `ContextCompactedEvent` through a projection, additive optional checkpoint ID/sequence is acceptable only if current serializers naturally carry it. Otherwise defer frontend projection to a later need.

### 6.6 Runtime and concurrency

M001 may wire production compaction to append the existing durable `ContextCompactedEvent` only if this can be done without pretending continuity installation already exists.

Preferred boundary:

- land store + event transaction foundation;
- keep existing compaction behavior unchanged;
- add tests proving the store can be called by M004.

If current runtime has a straightforward durable event emitter that is clearly missing, it may be corrected, but do not mark a checkpoint `Installed` from the old compaction path because no M002 payload contract exists yet.

### 6.7 Security

Checkpoint constructors must reject or never receive provider-hidden reasoning.

Add a bounded redaction test ensuring a fixture containing a secret-like tool argument is not automatically copied merely because a checkpoint envelope is serialized.

Store/log errors may include checkpoint/session IDs and digests, not payload bodies.

### 6.8 Documentation / static guards

Update:

- `architecture/session.md`
- `architecture/compaction.md`
- `architecture/context-compaction-ownership.md`
- `architecture/storage.md` if it enumerates tables/layout ownership.

A static guard is optional. Add one only if the repository already has an appropriate storage-version or ownership guard that can cheaply assert the new migration/store is wired.

## 7. Ordered work packages

### WP1 — Finalize typed checkpoint contract

Define lifecycle, identity, parent/sequence semantics, payload schema version, digest, and byte bounds.

Write unit tests for serialization, invalid status transitions, payload size rejection, and digest mismatch.

### WP2 — Add migration and store

Add the next free migration and layout-version bump. Implement prepare/get/latest/abort and migration tests from the immediately previous supported layout.

### WP3 — Atomic install + durable event

Implement the single-transaction install/event operation, stable event identity, idempotent retry, stale-parent rejection, and old-event deserialization compatibility.

### WP4 — Restart and contention tests

Open a database, prepare/install checkpoints, close/reopen, and prove:

- latest installed is recovered;
- prepared rows are ignored for resume;
- two same-parent contenders cannot both become the next installed checkpoint;
- duplicate install retry converges;
- corrupt/tampered digest fails.

### WP5 — Documentation and handoff surface

Document ownership and expose only the minimal store/types needed by M002/M004.

## 8. Failure, cancellation, restart, and contention semantics

M001 has no model/provider cancellation path. Store operations use transaction cancellation semantics from SQLx and leave no partially committed install/event pair.

A process crash:

- before `prepare` commit leaves no row;
- after `prepare` commit leaves a `Prepared` row that is not resumable;
- during install transaction rolls back both install state and event;
- after install commit exposes both `Installed` state and event.

On startup, stale `Prepared` rows may remain for diagnostics. Do not introduce a background cleanup task in M001.

Contention is session-scoped. The store must compare the expected previous installed checkpoint inside the same transaction that installs the candidate.

## 9. Compatibility and migration

Migration is additive. Existing sessions have no continuation checkpoints and therefore behave exactly as before.

Do not backfill synthetic checkpoints from old summaries or goal Markdown.

If implementation starts after storage v56 has advanced, rebase the plan to the next free migration number and update layout version accordingly. Do not create an out-of-order migration to preserve this document's v57 example.

## 10. Required tests

Focused unit tests:

- checkpoint status/schema serialization;
- payload bound;
- deterministic digest;
- invalid/tampered digest;
- state transition matrix.

Storage/integration tests:

- migration from pre-checkpoint layout;
- prepare/get/latest installed;
- prepared ignored by latest installed;
- install + event atomicity;
- duplicate install idempotency;
- stale-parent contention;
- abort semantics;
- reopen/restart recovery;
- old `ContextCompactedEvent` JSON deserialization.

Negative/security tests:

- oversized payload rejected before SQL insert;
- invalid session/checkpoint identifier rejected or safely parameterized;
- payload never appears in formatted diagnostic/log helper;
- event contains only bounded metadata.

## 11. Verification commands

Use the narrowest actual targets after implementation. Expected shape:

```text
cargo test -p codegg-core -- session
cargo test -p codegg-core -- continuation
python3 scripts/check_project_catalog_invariants.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Do not add a new CI lane.

## 12. Documentation updates

Document:

- checkpoint lifecycle and ownership;
- distinction from historical session checkpoints and goal Markdown checkpoints;
- storage schema/layout version;
- durable `ContextCompacted` event role;
- restart semantics;
- payload/digest bounds.

## 13. Acceptance criteria

M001 is complete only when:

1. a dedicated typed continuation checkpoint store exists under `codegg-core`;
2. its migration/layout bump is restart-safe and tested;
3. prepared, installed, and aborted states have explicit legal transitions;
4. only installed checkpoints are returned for continuation recovery;
5. checkpoint install and durable `ContextCompacted` event commit atomically;
6. stale-parent/concurrent installs fail deterministically;
7. duplicate install retry is idempotent;
8. payloads/digests are bounded and verified;
9. old session/event data remains readable;
10. no model-visible compaction strategy changed in this milestone;
11. focused tests and `scripts/verify.sh quick` pass.

## 14. Stop conditions

Stop and write an ADR/corrective plan before proceeding if:

- the checkpoint needs a new external service or database;
- the implementation would repurpose the historical `checkpoints` table incompatibly;
- `ContextCompactedEvent` cannot be extended compatibly;
- installing a checkpoint requires moving compaction policy into `codegg-core`;
- storage contention cannot be resolved with existing SQLite transaction policy;
- the payload must include hidden reasoning or secret-bearing raw tool data.

## 15. Closure evidence required

Closure must record:

- implementation commit(s);
- exact migration/layout version used;
- final table/index schema;
- lifecycle transition table;
- atomic install/event test evidence;
- restart and contention evidence;
- old-event compatibility evidence;
- payload bound/digest tests;
- documentation updates;
- verification commands/outcomes;
- any prepared-row cleanup intentionally deferred.

## 16. Handoff notes

M002 and M003 depend on the accepted store/type contract, not on private SQL details. Keep the public API small enough that those milestones can assemble/render/recover continuation state without importing `sqlx` into `src/context`.

Do not opportunistically implement M002 model-facing state or M003 handle expansion while landing M001. Their tests and security boundaries are intentionally separate.
