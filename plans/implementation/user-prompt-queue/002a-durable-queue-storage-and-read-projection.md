# User Prompt Queue and Turn Contention Milestone 002a — Durable Queue Storage and Read Projection

Status: ready for handoff

Repository baseline: `18c3dca92099fe2de09f875731530348fb76b68f` plus the M001
closure tree. M001 is closed (`plans/closure/user-prompt-queue/001-status.md`) but
**uncommitted at handoff**; re-inspect before implementing, because §3's line
numbers are anchored to the committed baseline and this milestone adds no TUI
submit-path behaviour.

Source roadmap: `plans/subsystems/user-prompt-queue-roadmap.md#7-milestones`
(Milestone 2, storage slice)

Long-term requirements:

- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#15-read-only-session-observation`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/001-terminology-and-domain-model.md`

Applicable ADRs:

- `plans/adrs/ADR-0035-durable-user-prompt-queue-ownership.md` (**accepted**
  2026-10-18)
- `plans/adrs/ADR-0037-idle-session-queue-mutation-authority.md` (**proposed**) —
  constrains what this slice may claim; see §5 and §14.

Primary class: **infrastructure**

## 1. Objective

Persist user prompt queue entries in a durable, bounded, expiring,
daemon-ordered table behind migration v80, and expose a typed, read-authorized
core request that returns them.

This milestone delivers machinery. **No user can queue a prompt when it closes.**
Every mutation operation is deliberately out of scope and lives in M002b, which
is gated on ADR-0037.

## 2. Why this milestone is ready

- **M001 closed** (`plans/closure/user-prompt-queue/001-status.md`). The submit
  path no longer corrupts active-turn state and restores composed text on every
  recoverable refusal, so the frontend's turn state is trustworthy to build on.
- **ADR-0035 accepted** (2026-10-18). Ownership, durability, bounds, admission
  discipline, and the read/mutate authority split are decided.
- **Storage substrate is stable and the migration path is proven.** v79 is the
  highest wired migration (`crates/codegg-core/src/session/schema.rs:352`), and
  `human_shell_promotion` (created in `migrate_v70`, `:2895`) is a working,
  in-tree analogue for a durable staged-prompt table with TTL, status, expiry,
  and typed bounds.
- **Read authority is a decided, existing seam.** `OperationDescriptor` with
  `ScopeKind::ViaSession` + `Capability::SessionRead` is already used by
  `session_selection_get` (`crates/codegg-core/src/authorization/policy.rs`).

The only open decision, ADR-0037, concerns **mutation** authority. Nothing in
this milestone mutates the queue, so this milestone is dependency-clean under
every ADR-0037 outcome.

## 3. Current implementation evidence

Anchored to the committed baseline. Re-inspect before editing.

**Storage.**
`crates/codegg-core/src/storage/mod.rs:41` declares
`pub const STORAGE_LAYOUT_VERSION: u32 = 79`.
`crates/codegg-core/src/session/schema.rs:333-352` is the monotone dispatch arm
list (`60 => … 79 => migrate_v79(&mut tx).await?`). Migration bodies are
`async fn migrate_vNN(tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>)`.
`scripts/check_project_catalog_invariants.py:292`
(`check_storage_layout_tracks_wired_migrations`) asserts the layout constant
equals the highest wired migration and has a `--self-test` covering both
mismatch directions. `tests/storage_migrations.rs:77,281` assert the same
equality from the root crate.

**The storage analogue.** `crates/codegg-core/src/session/human_shell_promotion.rs:11-14`:

```rust
pub const HUMAN_SHELL_PROMOTION_MAX_BYTES: usize = 32 * 1024;
pub const HUMAN_SHELL_PROMOTION_MAX_PENDING: i64 = 32;
pub const HUMAN_SHELL_PROMOTION_TTL_MS: i64 = 24 * 60 * 60 * 1000;
pub const HUMAN_SHELL_PROMOTION_MAX_RETAINED_PER_SESSION: i64 = 256;
```

Its DDL (`schema.rs:2895-2924`) is the shape to mirror: a `TEXT PRIMARY KEY`,
session scoping, a `CHECK`-bounded byte column, a `CHECK`-bounded status
vocabulary, timestamps, a nullable consumption binding, a compound `CHECK`
tying status to that binding, `FOREIGN KEY (session_id) REFERENCES session(id)
ON DELETE CASCADE`, and a covering index on the pending-selection columns.

**Authority (why ADR-0037 exists).**
`SessionControllerStore::acquire` (`session_control.rs:461`) validates a
non-empty `turn_id`; `release` (`:780`) `DELETE`s the row. So
`session_turn_controller` exists **only while a turn runs**.
`CoreDaemon::controller_lease_for` (`src/core/daemon_control.rs:85`) returns
`None` when there is no row, and `check_turn_controller` (`:124`) then refuses
**every** principal. The single idle-passing path is the bypass at `:74`,
`is_controller_bypass` → `authorization::is_local_owner_broad`, an
identity-only comparison against `LOCAL_OWNER_PRINCIPAL_ID` that is **not bound
to a session**. This slice must not rely on it.

**Protocol seams.**
`crates/codegg-protocol/src/core.rs`: `CoreRequest::HumanShellPromotionStage`
(`:2284`) and `HumanShellPromotionsGet` (`:2290`);
`CoreResponse::HumanShellPromotionStaged` (`:1192`) and `HumanShellPromotions`
(`:1196`); `HumanShellPromotionDto` (`:1922`) carries the bounded metadata and
explicitly never carries raw shell output.
Every new request must be registered in four places, none optional:
`CoreDaemon::session_id_for_request` (`src/core/daemon.rs:3042`, for the audit
chain's session locator), the family map (`src/core/daemon_family.rs:138-142`,
`HumanShellPromotion* => Self::Turns`), the turn dispatcher
(`src/core/daemon_turns.rs`), and `operation_descriptor`
(`crates/codegg-core/src/authorization/policy.rs:381`).

**Audit classification.**
`crates/codegg-core/src/audit_instrumentation.rs`: `turn_submit`,
`session_prompt_submit`, `human_shell_promotion_stage`, and `turn_steer` all
map to the `prompt_submit` audit action (`:561`). `human_shell_promotions_get`
is listed in `UNINSTRUMENTED_OPERATIONS` (`:712`) — **reads go there**, and
`scripts/check_audit_coverage.py` fails a mutating-capability operation that is
in neither list.

**Frontend read seam.**
`App::load_human_shell_promotions` (`src/tui/app/mod.rs:2571`) is the existing
one-shot fetch: build the request, spawn a tracked task, match the typed
response, and hand a typed command back to the loop. It uses
`task_registry.spawn(TuiTaskKind::Command, …)` rather than `spawn_tui_task`;
either shared guarded shape is acceptable, a raw `tokio::spawn` is not.
`is_observer_allowed_command` (`src/tui/app/state/observe.rs:476`) is a narrow
fail-closed read-only allowlist — a new command is **blocked** by omission, which
is the safe default for a read.

**Known gap carried in.** M001's closure records that the `agents_to_dtos`
pre-flight failure still does not release the turn. It is unrelated to this
milestone; do not absorb it here.

## 4. Invariants that must not regress

- **`codegg-core` stays UI-, server-, plugin-, and auth-free.** New storage code
  belongs in `crates/codegg-core/src/session/` and is enforced by
  `scripts/check-core-boundary.sh`.
- **`STORAGE_LAYOUT_VERSION` equals the highest wired migration**, asserted by
  two independent checks.
- **Storage changes are additive.** No existing table is altered and no stored
  data is rewritten.
- **Prompt text never reaches audit records, receipts, diagnostics, or error
  messages verbatim.** Queue entries are user-authored prompt content with the
  same sensitivity as a submitted turn.
- **A bounded resource refuses rather than truncates.** Exceeding a count or
  length bound is a typed refusal, never a silent shortening that changes what
  the user wrote.
- **Entry identity is the durable unit.** Audit and diagnostics reference entry
  identity, never entry text.
- **No queue path acquires a scheduler permit, executor ownership, or workspace
  execution context** (ADR-0035 Option C).
- **Reads do not mutate.** This milestone's only core request is a read.

## 5. Scope

### In scope

- Migration **v80** creating `user_prompt_queue_entry`, plus the
  `STORAGE_LAYOUT_VERSION` bump and a new dispatch arm.
- The entry model: identity, session, daemon-owned ordinal, bounded text,
  status, optional failure reason, created/updated/expiry timestamps.
- Bounds as named constants with typed refusals **and** database `CHECK`
  backstops.
- Lazy, deterministic expiry evaluation.
- `CoreRequest::UserPromptQueueList` → `CoreResponse::UserPromptQueue` plus
  `UserPromptQueueEntryDto`.
- `OperationDescriptor` with `ScopeKind::ViaSession` and
  `Capability::SessionRead`; `UNINSTRUMENTED_OPERATIONS` classification.
- Daemon read handler, family mapping, and `session_id_for_request` registration.
- The TUI read path: a seeded projection field, a load on session change, and a
  typed completion command.
- Documentation and the layout/audit guards.

### Explicitly out of scope

- **Every mutation**: add, edit, delete, reorder. These are M002b and are gated
  on ADR-0037.
- Any second submission path. A queue entry becomes a turn only through ordinary
  `TurnSubmit` (ADR-0035), which is M003.
- `run_at_ms` and delayed delivery — M007. See §6 for why the column is omitted.
- The `/queue` dialog, row 0 suffix, or any user-visible surface — M004.
- `TurnSteer` wiring — M006, gated on ADR-0036.
- **Any claim that a user can queue a prompt.** This milestone is infrastructure
  and its closure record must say so in those words.

## 6. Required production changes

### Core/domain

Add `crates/codegg-core/src/session/user_prompt_queue.rs` exporting a
`UserPromptQueueStore` in the shape of `SessionControllerStore`: constructed from
a `SqlitePool`, exposing read and (later) mutation operations that all return
typed `Result<_, UserPromptQueueError>`. Declare the module from
`crates/codegg-core/src/session/mod.rs`.

`UserPromptQueueError` follows the `SessionControlError` pattern: an `Invalid`
variant for validation failures carrying `field` + `message`, typed bound
variants (`TooManyEntries`, `EntryTooLarge`, `NotFound`, `Expired`), and a
`code()` ladder in the same style as `SessionControlError::code` /
`SessionTurnAdmissionError::code` so a daemon handler can map a variant to a
`CoreResponse::Error` code without an intermediate stringly-typed layer.

### Storage and migrations

Add `migrate_v80` to `crates/codegg-core/src/session/schema.rs` and the
`80 => migrate_v80(&mut tx).await?` dispatch arm. Bump `STORAGE_LAYOUT_VERSION`
to 80.

The table mirrors `human_shell_promotion`:

```sql
CREATE TABLE IF NOT EXISTS user_prompt_queue_entry (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    text TEXT NOT NULL,
    text_bytes INTEGER NOT NULL CHECK (text_bytes >= 0 AND text_bytes <= <MAX_ENTRY_BYTES>),
    status TEXT NOT NULL CHECK (status IN
        ('pending', 'delivering', 'delivered', 'failed', 'expired')),
    failure_reason TEXT,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    expires_at_ms INTEGER NOT NULL,
    delivered_turn_id TEXT,
    delivered_at_ms INTEGER,
    FOREIGN KEY (session_id) REFERENCES session(id) ON DELETE CASCADE
);
```

plus a covering index on `(session_id, status, ordinal)`.

**Two decisions this plan fixes, and their reasons.**

1. **The status `CHECK` enumerates all five roadmap values even though only
   `pending` and `expired` are reachable in this milestone.** SQLite cannot
   `ALTER` a `CHECK` constraint; narrowing it now and widening it in M003 would
   force a create-copy-drop-rename rebuild of a durable user table — a far larger
   compatibility event than ADR-0035 contemplated. A `CHECK` value is a
   validation constraint, not a code path: it cannot execute wrongly. It is not
   the "variant with no producer" hazard AGENTS.md warns about. The cost is that
   three values are temporarily unreachable, which the closure record must name as
   a bounded finding owned by M003, and M003 must either make all five reachable
   or rebuild the table — a decision M003 inherits explicitly.
2. **`run_at_ms` is omitted from v80.** ADR-0035's Option B description includes
   an optional delayed-delivery column, but nothing in this milestone writes it,
   and a durable column with no producer is exactly the claim-without-a-hook
   failure this repository treats as a defect. M007 adds it as a nullable column
   in **v81** via `ALTER TABLE … ADD COLUMN`, which SQLite performs in place with
   no rebuild. This is a deliberate, recorded narrowing of the ADR's descriptive
   table sketch, not a silent one.

**Ordering.** Ordinal is daemon-maintained and dense per session. Add a
`UNIQUE (session_id, ordinal)` constraint so the total order is a schema-enforced
invariant rather than a convention, and record in the closure record that
reorder (M002b) must therefore renumber under one transaction using a
collision-free strategy. This slice only ever appends at
`max(ordinal) + 1`, so the constraint is satisfiable from day one and the
reorder design constraint is inherited, not invented here.

**Bounds.** New constants alongside the promotion precedent, with values chosen
for a *user prompt* rather than a derived shell projection:

```rust
pub const USER_PROMPT_QUEUE_MAX_ENTRY_BYTES: usize = 64 * 1024;
pub const USER_PROMPT_QUEUE_MAX_PENDING: i64 = 64;
pub const USER_PROMPT_QUEUE_TTL_MS: i64 = 7 * 24 * 60 * 60 * 1000;
pub const USER_PROMPT_QUEUE_MAX_RETAINED_PER_SESSION: i64 = 256;
```

Justification the closure record must confirm rather than restate: 64 KiB is
comfortably above any realistic prompt and far below the product's one-time 1 MiB
turn bound, so the queue does not become a way to park megabytes; 64 pending
entries is a backlog, not an archive; 7 days outlives the 24-hour promotion TTL
because a queue is an intentional backlog rather than a staging buffer, and an
overnight instruction must survive to the morning. `MAX_RETAINED_PER_SESSION`
mirrors the promotion precedent for pruning terminal-state rows. If any value is
adjusted, record the measured reason.

Every bound is enforced **twice**: a typed refusal at the store boundary (the
primary mechanism) and a `CHECK` constraint in the schema (the backstop that
survives a missed call site).

### Protocol and DTOs

In `crates/codegg-protocol/src/core.rs` add:

```rust
CoreRequest::UserPromptQueueList { session_id: String }
CoreResponse::UserPromptQueue { session_id: String, entries: Vec<UserPromptQueueEntryDto> }
pub struct UserPromptQueueEntryDto { /* id, ordinal, status, text_bytes, created_at_ms, expires_at_ms, … */ }
```

`UserPromptQueueEntryDto` carries the **text** (the user needs to see and later
edit it — M005 owns editing) plus bounded metadata. Mark every new field
`#[serde(default)]` so a projection written by a newer daemon is readable by an
older client and an absent projection reads as an empty queue, exactly as
ADR-0035 requires.

### Runtime and concurrency

- Register the request in `session_id_for_request` (`src/core/daemon.rs:3042`) so
  the audit chain carries the session locator.
- Map it to `Self::Turns` in `src/core/daemon_family.rs`.
- Add the handler arm in `src/core/daemon_turns.rs` that reads the pool and
  returns the DTO list.
- Add the `OperationDescriptor` in
  `crates/codegg-core/src/authorization/policy.rs` with
  `ScopeKind::ViaSession` and `Capability::SessionRead`.
- Add `"user_prompt_queue_list"` to `UNINSTRUMENTED_OPERATIONS`.

**Note the deliberate divergence from the promotion precedent.**
`human_shell_promotions_get` is authorized with `Capability::AgentInvoke` even
though it is a read. This new read uses `SessionRead`, because ADR-0035 states
"queue reads are subject to session read authority". Record the divergence in the
closure record rather than copying the older, broader capability.

Reads evaluate expiry lazily: the read path marks due entries `expired` and
returns the survivors, in one transaction. This is M002's share of ADR-0035's
"expiry is evaluated lazily and deterministically", and it gives `expired` a real
producer in this milestone.

### Frontend or operator surface

Add a seeded projection field to the TUI session state, populated by a load on
session change following the `load_human_shell_promotions` shape
(`src/tui/app/mod.rs:2571`): build the request, spawn through one of the shared
guarded task shapes, match the typed response, send a typed completion command.
No dialog, no key, no row 0 change — this is not a user-visible surface yet, and
adding one would make infrastructure look like capability.

Do not add `/queue` to `is_observer_allowed_command`. Omission is fail-closed and
correct here; the M004 milestone owns that registration.

### Security and authorization

- Read authority is `SessionRead` via the existing descriptor seam; do not
  hand-roll an authority check in the handler.
- **Do not** extend `is_controller_bypass` or otherwise route the read through
  the local-owner bypass. A read needs session read authority, not an identity
  bypass.
- No mutation request variant exists in this milestone, so there is no mutation
  authority to get wrong.
- The entry text must not appear in any `tracing` call, audit record, or
  `CoreResponse::Error` message.

### Documentation and static guards

`architecture/session.md` gains the queue storage model; `architecture/core.md`
gains the queue read contract and states that it is read-only infrastructure with
no mutation surface yet; `architecture/authorization.md` gains the new
descriptor row. No TUI documentation changes in this milestone — there is no
user-visible behaviour to describe, and `docs/tui.md` must not grow a section for
a feature nobody can use.

The existing guards are the contract: `check_project_catalog_invariants.py` for
the layout constant and `check_audit_coverage.py` for the operation
classification. Both already run in `verify.sh quick`; this milestone needs no new
guard script.

## 7. Ordered work packages

### Work package A — Migration v80 and the entry model

Intent: create the table, bump the layout constant, prove both assertions, and
give every column a producer.

Required changes: `migrate_v80`; the dispatch arm; `STORAGE_LAYOUT_VERSION = 80`;
the four bounds constants; the store's `new`, `pool`, and row-mapping code in the
`human_shell_promotion` style, including the `text_bytes` `TryFrom` failure path
that precedent handles at `:151`.

Acceptance evidence: `cargo test --test storage_migrations` green;
`python3 scripts/check_project_catalog_invariants.py` green including its
`--self-test`; a v79→v80 migration test that opens a v79 pool, migrates, and
asserts the table exists with the expected columns and that the layout equals 80.

### Work package B — Bounds as typed refusals

Intent: prove every bound refuses with a typed error rather than truncating.

Required changes: store-level enforcement of entry bytes, pending count, and
per-session retention; typed error variants and the `code()` ladder.

Acceptance evidence: one test per bound asserting the exact typed variant — not a
generic `Err` — plus a test asserting stored text is byte-identical to what was
submitted at the boundary, so truncation would fail rather than pass quietly.
Also assert the schema `CHECK` rejects an over-long `text_bytes` written
directly, proving the backstop is real and not decorative.

### Work package C — Lazy deterministic expiry

Intent: give `expired` a producer and prove expiry survives restart.

Required changes: the read path's transactional expiry sweep.

Acceptance evidence: a test that an entry past `expires_at_ms` reads back as
absent from the live list and carries `status = 'expired'` in a direct store
read; a test that expiry is idempotent across repeated reads; a test that a
non-expired entry is untouched.

### Work package D — Typed read request, descriptor, and daemon handler

Intent: expose the projection through the existing authorized seam without a
second live-delivery path.

Required changes: the protocol variants and DTO, `session_id_for_request`, the
family map, the handler arm, the `OperationDescriptor`, and the
`UNINSTRUMENTED_OPERATIONS` entry.

Acceptance evidence: a handler test asserting the typed response shape; an
authorization test asserting a principal without `SessionRead` is refused for the
read; `scripts/check_audit_coverage.py` green; a guard test asserting no mutation
variant exists — a cheap way to make M002b's boundary visible rather than
implied.

### Work package E — TUI read path

Intent: keep the frontend's view of the queue derived from the daemon, never
authoritative.

Required changes: the seeded projection field and its load, following
`load_human_shell_promotions`.

Acceptance evidence: a test asserting a session change triggers exactly one load
and an absent response yields an empty list rather than an error toast; a test
that the failure path leaves the previous value intact instead of clearing it.

## 8. Failure, cancellation, restart, and contention semantics

- **Daemon restart:** every entry is durable. A restart loses nothing, changes no
  status, and expires nothing that is not already due. Nothing is replayed — this
  milestone never submits anything.
- **Frontend restart:** the TUI re-reads the queue on session change. It never
  replays a local copy, because it has none.
- **Partial failure:** a mutation-free milestone has no partial-write surface;
  the expiry sweep runs inside one transaction so a crash mid-sweep leaves either
  all or none of the due entries expired.
- **Concurrent readers:** both observe the same daemon-owned order. Ordinals are
  assigned by the daemon under transaction, so a reader never sees a gap or a
  duplicate.
- **Stale generation:** the TUI load uses the shared guarded task shapes with a
  `finish`/`fail` guard, so a completion for a superseded session cannot write
  into current state.
- **Cancellation:** cancelling a load discards its completion; it must not clear
  the previous projection.
- **Duplicate delivery:** not reachable in this milestone. The at-most-once
  discipline belongs to M003 and is recorded there.

## 9. Compatibility and migration

- **Additive only.** One new table, one new index, one new constant value. No
  existing table is altered; no stored data is rewritten.
- **`STORAGE_LAYOUT_VERSION` 79 → 80**, asserted by
  `scripts/check_project_catalog_invariants.py` and `tests/storage_migrations.rs`.
- **Protocol additions are new variants**, additive on the wire. A client that
  predates them never issues them, so no negotiation is needed.
- **An absent projection reads as an empty queue**, per ADR-0035. New DTO fields
  are `#[serde(default)]`.
- **No configuration key** is introduced; there is nothing for a user to enable.
- **Removal criteria:** none. Once shipped this table is a compatibility
  obligation, like every other migration in the chain.

## 10. Required tests

Name every new test with a `user_prompt_queue` prefix so the §11 filters select
real tests. (M001's closure recorded a plan whose filters were module paths and
matched zero tests; do not repeat that.)

### Focused unit tests

- Row mapping, including the `text_bytes` conversion failure path.
- Bound constants' refusal variants, one test each.
- `code()` ladder: each `UserPromptQueueError` variant maps to a distinct,
  stable code string.

### Integration tests

- v79 → v80 migration with the table and index present.
- Round-trip: insert, read back byte-identical, delete by id.
- `SessionRead`-authorized read returns the daemon order.
- A principal without `SessionRead` is refused.

### Restart and recovery tests

- Reopen a pool after insert; entries, ordinals, and status survive unchanged.
- Due entries expire across a simulated restart; non-due entries do not.

### Contention and cancellation tests

- Two concurrent readers observe an identical order with no gaps or duplicates.
- A cancelled TUI load leaves the previous projection intact.

### Security and negative tests

- Prompt text appears in no audit record and no error message. Assert on the
  record, not on the absence of a log line.
- Over-bound `text_bytes` written directly is rejected by the schema `CHECK`.
- No `CoreRequest` queue-mutation variant exists (M002b boundary guard).

### Migration and compatibility tests

- `STORAGE_LAYOUT_VERSION == 80` and equals the highest wired migration.
- `scripts/check_project_catalog_invariants.py --self-test` still detects both
  mismatch directions.

## 11. Required verification commands

```bash
# formatting
cargo fmt

# new storage work — the prefix is a TEST-NAME filter, not a module path
cargo test -p codegg-core user_prompt_queue

# the analogue must be untouched and still green
cargo test -p codegg-core --test human_shell_promotion

# migration and layout assertions (root crate)
cargo test --test storage_migrations

# authorization/identity sweep
cargo test -p codegg-core --test identity

# static guards
python3 scripts/check_project_catalog_invariants.py
python3 scripts/check_project_catalog_invariants.py --self-test
python3 scripts/check_audit_coverage.py
bash scripts/check-core-boundary.sh

# lint
cargo clippy -p codegg-core --all-targets -- -D warnings
cargo clippy -p codegg --all-targets -- -D warnings

# canonical sweep
scripts/verify.sh quick
```

Use `CARGO_BUILD_JOBS=2`; the workspace build is long. Do not use
`--all-features` — it drags in real-server tests.

**Report only commands actually run, with real counts.** If a filter selects zero
tests, say so and state what covered the area instead. Do not claim hosted CI.

## 12. Documentation updates

- `architecture/session.md` — the queue table, bounds, ordinal ordering, and lazy
  expiry; cross-reference ADR-0035.
- `architecture/core.md` — the read contract, its authorization, and an explicit
  statement that no mutation surface exists yet.
- `architecture/authorization.md` — the new descriptor row and the deliberate
  `SessionRead`-not-`AgentInvoke` divergence from `human_shell_promotions_get`.
- No `docs/tui.md` change. There is no user-visible behaviour.
- No change to `AGENTS.md` or the skills index.

## 13. Acceptance criteria

1. A v79 pool migrates to v80; the table, index, and `CHECK` constraints exist;
   `STORAGE_LAYOUT_VERSION` equals 80 and both assertions pass.
2. Entries survive process restart with byte-identical text, unchanged ordinal,
   and unchanged status.
3. Entry count, per-entry length, and per-session retention each produce their
   **specific** typed refusal at the store boundary, and the schema rejects an
   over-bound row written directly.
4. A due entry is `expired` and absent from the live list; a non-due entry is
   untouched; repeated reads are idempotent.
5. `UserPromptQueueList` returns entries in daemon-owned dense ordinal order and
   is refused for a principal without `SessionRead`.
6. The operation is classified in `UNINSTRUMENTED_OPERATIONS`, and
   `scripts/check_audit_coverage.py` passes.
7. No `CoreRequest` variant can mutate the queue, and a guard test asserts it.
8. `codegg-core` remains free of UI, server, plugin, and auth dependencies.
9. **The closure record states plainly that no user can queue a prompt.**

## 14. Stop conditions

Stop and report rather than improvise when:

- **ADR-0037 is still `proposed` and the work appears to require a mutation
  authority decision.** It does not. Do not implement add, edit, delete, reorder,
  or ownership. If any of them appears necessary, that is the stop signal.
- The existing `human_shell_promotion` DDL or bounds turn out to be a worse
  analogue than assumed — report it rather than inventing a third shape.
- `check_project_catalog_invariants.py` or `check_audit_coverage.py` fails for a
  reason this plan did not predict; do not weaken the guard.
- The layout constant is already **80 or higher** at implementation time (another
  milestone landed first). Re-derive the migration number from the current
  dispatch arm rather than forcing v80.
- Making the read work appears to require a second live-delivery path or a
  frontend-owned authoritative copy. ADR-0035 forbids both.
- A test asserting "no prompt text in diagnostics" cannot be written against the
  actual record shape; report the gap instead of weakening the assertion.
- Scope would expand into M002b, M003, M004, or M007.

## 15. Closure evidence required

The closure record must contain:

1. Migration output: the v79→v80 test result and both layout assertions.
2. Every bound test with its exact typed variant named, plus the truncation
   proof.
3. Restart, expiry-idempotence, and concurrent-reader results.
4. The authorization refusal result for the read.
5. The `check_audit_coverage.py` and `check_project_catalog_invariants.py`
   results, including `--self-test`.
6. Both clippy results and `verify.sh quick`.
7. A statement that hosted CI was **not** run, if it was not.
8. The finding that `delivering`, `delivered`, and `failed` are unreachable until
   M003, and the explicit inheritance of the rebuild-or-widen decision.
9. The recorded narrowing that `run_at_ms` was omitted and why.
10. The recorded divergence from the promotion read's `AgentInvoke` capability.
11. **An explicit statement that this milestone delivers no user-visible
    capability**, and the ADR-0037/M002b handoff.
12. The unblock audit: confirm M002b remains blocked on ADR-0037 and that nothing
    else changed status.

## 16. Handoff notes

- **Class discipline.** This is infrastructure. `verify.sh quick` passing is not
  evidence a user can queue anything, and the closure record must not read as
  though it is.
- **The read has no producer yet**, by construction. The queue will always read
  empty until M002b lands. That is expected and must be stated, not hidden.
- **Do not extend `is_controller_bypass`.** ADR-0037 exists precisely because the
  bypass is identity-only and session-unbound. Touching it here would pre-empt an
  accepted-but-undecided architecture question.
- **SQLite cannot alter a `CHECK`.** If M002b or M003 wants a different status
  vocabulary, it costs a table rebuild; raise it rather than assuming it is
  cheap.
- **Serialization.** `codegg-core` storage tests mutate process-global state
  patterns in some suites; prefer `isolated_pool()` and follow the existing
  convention in the crate rather than inventing a new one.
- **Build cost.** `CARGO_BUILD_JOBS=2`. Workspace builds run for minutes per
  target; use background execution for long sweeps.
- **Preserved work.** M001's changes are uncommitted in the working tree. Do not
  stash, revert, or reformat them.