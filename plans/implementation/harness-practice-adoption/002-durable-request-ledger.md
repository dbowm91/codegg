# Harness Practice Adoption Milestone 002 — Durable Request Ledger

Status: conditionally closed

Repository baseline: `7642b9e381d9c91d650fa375be2a7aa0aae9bcd8`
(branch `feat/auto-20261109-f8abad0d`)

Source roadmap:

- `plans/subsystems/harness-practice-adoption-roadmap.md` §7 M002, §8

Long-term requirements:

- `plans/002-long-term-roadmap.md#phase-5--frontend-neutral-session-projections-and-durable-replay`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
  — bounded payloads, restart-safe migrations
- `plans/000-long-term-specification.md#28-observability`
- `plans/001-terminology-and-domain-model.md#12-audit-and-observability-terms`
- `plans/003-planning-process.md`

Applicable ADRs:

- **None as written.** The record is an additive session-store row with no new
  `CoreRequest` variant and no new field on the canonical session projection.
  §14 makes that a stop condition: if the record cannot be expressed without
  either, the agent stops and an ADR is opened.

Primary class: infrastructure

## 1. Objective

Every model request **attempt** durably records the identity of what was sent —
the system prompt's digest, the admitted tool declarations' digest, the message
range's digest, and the model/effort selection — so that "what did the model
see in this turn?" is answered from storage rather than reconstructed from
today's assets.

## 2. Why this milestone is ready

- No hard dependency. The roadmap records the line as free of them.
- The write seam exists and already carries attempt identity:
  `src/agent/provider_turn.rs` retries a request up to `MAX_ATTEMPTS` and its
  attempt function receives an `attempt_id`.
- The digest of the tool declarations already has a producer:
  `src/context/tool_hash.rs` provides `tool_definitions_hash` for cache
  stability. This milestone reuses it rather than adding a second definition
  fingerprint.
- The storage convention is established: an additive `migrate_vNN` in
  `crates/codegg-core/src/session/schema.rs`, `STORAGE_LAYOUT_VERSION` bumped in
  the same change, and `scripts/check_project_catalog_invariants.py` enforcing
  that the two agree.
- M001 is not a hard dependency, but it is the cheapest acceptance test for
  this milestone and the roadmap sequences it first for that reason.

## 3. Current implementation evidence

**Messages are persisted; the request is not.** The session store persists
conversation rows. A search of `crates/codegg-core/src/session` finds no
`system_prompt` column and no request table. The assembled prompt is rebuilt
each turn from assets and configuration (`src/agent/prompt.rs` builds
`PromptBlock` sections), so the same turn re-run after an asset edit sends a
different request with no record that it changed.

**The dispatch seam is a single function with retries.**
`src/agent/provider_turn.rs` wraps `provider.stream(request)` in
`STREAM_SETUP_TIMEOUT` and `STREAM_IDLE_TIMEOUT`, retries up to
`MAX_ATTEMPTS`, and already threads an `attempt_id` into the attempt call. It
also already reports a terminal outcome through an observer, so an
attempt-scoped side effect at this seam has precedent.

**The tool fingerprint exists.** `src/context/tool_hash.rs` computes a stable
hash over tool definitions for cache purposes. It is not persisted per request.

**The context ledger is a different record.** `architecture/context-ledger.md`
records artifact packing — which blocks were included, omitted or projected —
not the identity of the request that resulted. The two are complementary; this
milestone does not extend the artifact store.

**Migrations are at v94.** `STORAGE_LAYOUT_VERSION` is `94` and the highest
migration in `crates/codegg-core/src/session/schema.rs` is `migrate_v94`. This
milestone adds `migrate_v95`.

**A read-before-write shape has a known defect class.** A queue write path that
reads before it writes must open its transaction with `begin_immediate`;
`scripts/check_queue_write_transaction_shape.py` exists because a deferred
transaction's first `SELECT` holds a shared lock and the first `UPDATE` cannot
upgrade it without waiting, so concurrent callers fail with `SQLITE_BUSY`
instead of one winning. This milestone's write path has the same shape and
follows the same rule.

## 4. Invariants that must not regress

- **The ledger does not change what is sent.** No request field, ordering,
  timeout, retry count or provider selection changes. A byte-level request
  comparison before and after is the proof.
- **One writer.** Exactly one production function writes a ledger row, and a
  guard names it. A second writer is how two rows for one attempt become
  possible.
- **The digest is over the dispatched value.** It is computed from the
  `ChatRequest` that is passed to `provider.stream`, not from a rebuild. A
  digest computed independently is a second source of truth that can disagree
  with the thing it claims to describe.
- **The digest is not over redacted content.** The stored body is redacted at
  write; the digest is over what was actually sent. This is deliberate: a digest
  over redacted content would stop identifying the request. §16 records the
  consequence — a reader cannot recompute the digest from the stored body.
- **No secret material is stored.** Bodies are redacted at write under the same
  rule the delegated transcript uses, and a body is stored only when capture is
  enabled or the attempt failed.
- **Retention is bounded and enforced by the writer.** A long-lived session
  cannot grow the ledger without limit.
- **The migration is restart-safe and additive.** It creates a table and an
  index; it does not alter or drop an existing one.

## 5. Scope

### In scope

- One additive migration: a `turn_request_ledger` table with a uniqueness
  constraint over `(session_id, turn_index, attempt_index)` and an index for the
  per-session read.
- One write at the attempt seam in `src/agent/provider_turn.rs`.
- One reader in the session store, with two consumers: a production read-only
  diagnostic and M001's runner.
- A per-session row bound enforced inside the write transaction.
- A guard naming the single writer, with a self-test.

### Explicitly out of scope

- A `CoreRequest` variant, a protocol field, or a projection field. §14.
- Storing a body by default. Bodies are opt-in or failure-only.
- Reconstructing a request from the ledger. It records identity, not a replay
  source; M001 replays from fixtures.
- Any change to the context ledger, the artifact store, or the compaction
  policy.
- A UI surface. The production consumer is a diagnostic, not a panel.
- Retroactive backfill. Rows begin at the migration; earlier turns have none,
  and the reader reports that honestly rather than inventing them.

## 6. Required production changes

### Core/domain

- A `TurnRequestRecord` type in the session domain with the fields in §7
  work package A, and a store method to read rows for a session and turn.
- The digest helpers: reuse `tool_definitions_hash` for the tool digest; add a
  canonical digest for the system prompt (over the ordered section kinds and
  their content) and for the message range (over ordered role and content).
  The canonical form is specified, not incidental: two implementations that
  disagree produce a false "the prompt changed".

### Storage and migrations

- `migrate_v95` creating the table and its index, and `STORAGE_LAYOUT_VERSION`
  bumped to `95`.
- The write path opens with `begin_immediate` because it reads the current row
  count before it inserts and prunes.
- The reader is a single query ordered by `(turn_index, attempt_index)`.

### Protocol and DTOs

None. The record is daemon-internal. If a frontend needs it, that is a
projection decision and an ADR.

### Runtime and concurrency

- The write happens inside the attempt function, before the stream is consumed,
  so a request that fails at setup still records an attempt.
- The write must not delay dispatch: it is awaited before the provider call
  because a row that appears after a crash is not a record of what was sent.
  The measured cost is a single indexed insert; the plan requires it to be
  reported in the closure rather than assumed negligible.
- A cancelled turn that never reaches the attempt writes no row. A turn
  cancelled after dispatch writes a row whose outcome is `cancelled`.

### Frontend or operator surface

- One read-only diagnostic. `codegg doctor` already dispatches by subsystem
  (`DoctorSubsystem`); a `session` subsystem reports the ledger's health for the
  current store: row count, the last row's digests, and whether a body is
  present. It prints digests, never bodies.

### Security and authorization

- Bodies are redacted at write.
- The diagnostic requires the same store access as `doctor` already has and
  adds no capability.
- The digest is over the whole request, not per field, so it does not create a
  per-field oracle over a short secret.

### Documentation and static guards

- `architecture/session.md` gains the ledger: the table, the one writer, the
  retention bound, and the digest-versus-body distinction.
- `architecture/agent.md` gains the attempt seam's responsibility.
- `scripts/check_request_ledger_writers.py` with `--self-test`.

## 7. Ordered work packages

### Work package A — The record and its digests

Intent: define identity before storing it.

Required changes:

- Table `turn_request_ledger`:
  `id`, `session_id`, `turn_index`, `attempt_index`, `attempt_id`, `model`,
  `effort` (nullable), `system_digest`, `tools_digest`, `message_digest`,
  `message_count`, `section_digests` (nullable JSON map from section kind to
  digest), `outcome` (`streaming` | `ok` | `failed` | `cancelled`),
  `body` (nullable, redacted), `created_at`.
  Unique over `(session_id, turn_index, attempt_index)`.
- A canonical serialization for each digest input, documented in the code that
  owns it. The system digest covers ordered section kinds and content; the
  message digest covers ordered role and content; the tool digest is the
  existing `tool_definitions_hash`.
- `migrate_v95` and `STORAGE_LAYOUT_VERSION = 95`.

Acceptance evidence:

- A focused test proves each digest is stable across two identical inputs and
  changes when one byte of the input changes.
- A migration test runs from the previous layout version and finds the table
  and index.
- `scripts/check_project_catalog_invariants.py` is green.

### Work package B — The single writer

Intent: one row per attempt, written before dispatch, with a bounded table.

Required changes:

- One function that takes the `ChatRequest`, the turn identity, the attempt
  identity, the model/effort selection and the section digests, and writes one
  row.
- Called from the attempt function in `src/agent/provider_turn.rs`, before
  `provider.stream(request)`.
- The transaction opens with `begin_immediate`; inside it the writer counts the
  session's rows and prunes the oldest beyond
  `MAX_REQUEST_LEDGER_ROWS_PER_SESSION` before inserting.
- The outcome column is updated once the attempt settles, through the same
  store handle, keyed on the row's unique identity — not by a second insert.

Acceptance evidence:

- A turn writes exactly one row per attempt; a retried attempt writes a second
  row with the next `attempt_index`.
- A turn that fails at stream setup still has a row whose outcome is `failed`.
- A session over the bound holds exactly the bound, oldest pruned.
- Two concurrent attempts on different sessions do not contend for one lock
  (proven by a test that runs both and asserts neither returns a busy error).

### Work package C — The reader and its production consumer

Intent: the row is read by something that ships, not only by a test.

Required changes:

- A store method returning the rows for `(session_id, turn_index)` ordered by
  attempt.
- `codegg doctor` gains a `session` subsystem that reports the ledger's health
  and the last row's digests. Digests only; never a body.
- M001's runner uses the same reader to compare a scenario's request identity
  when a scenario declares it. This is additive to M001, not a change to it.

Acceptance evidence:

- The diagnostic prints the expected row count and digests for a store with
  rows, and reports "no ledger rows" for a store without them.
- The diagnostic output contains no body content, proven by a test that seeds a
  body with a marker string and asserts it is absent from the output.

### Work package D — The guard

Intent: the single-writer rule and the retention bound cannot rot.

Required changes:

- `scripts/check_request_ledger_writers.py` verifies: exactly one production
  call site writes a ledger row; the write site is inside the attempt function;
  the retention bound constant exists and is referenced by the writer; the
  reader has at least one non-test consumer; and the table name appears in the
  migration chain.
- `--self-test` injects a second writer and proves rejection, injects a missing
  bound and proves rejection, and injects a reader with only a test consumer and
  proves rejection.

Acceptance evidence:

- All three injections are rejected.
- The guard is green on the implementation.

## 8. Failure, cancellation, restart, and contention semantics

- **Failure before dispatch.** No row. There was no request.
- **Failure during stream setup.** One row, outcome `failed`.
- **Failure mid-stream.** One row, outcome `failed`, with the body retained
  because the attempt failed.
- **Cancellation before dispatch.** No row.
- **Cancellation after dispatch.** One row, outcome `cancelled`.
- **Retry.** Each attempt is its own row. The `attempt_index` distinguishes
  them; the unique constraint makes a duplicate attempt impossible rather than
  merely unlikely.
- **Restart.** Rows are durable. A restart mid-attempt leaves the row's outcome
  as `streaming`, and the reader reports that as an interrupted attempt rather
  than as success. This is stated because a `streaming` row is the honest
  record of a process that died.
- **Contention.** Two attempts in one session cannot collide on the unique
  constraint because the attempt index is monotonic within a turn. Two sessions
  contend only on the store's write lock, and `begin_immediate` makes one wait
  rather than fail.
- **Store unavailable.** The write failure must not fail the turn: the request
  is more important than its record. A write failure is logged and the turn
  proceeds. This is a deliberate priority, and §16 says so.

## 9. Compatibility and migration

- The migration is additive and restart-safe, tested from the previous layout
  version.
- An older daemon reading a newer store is refused by the existing layout-version
  check; no new negotiation is introduced.
- No protocol, configuration, or dependency change.
- The diagnostic is additive: an older store without the table reports "no
  ledger rows" rather than failing.

## 10. Required tests

### Focused unit tests

- Each digest is stable and changes on a one-byte change.
- The canonical serialization is order-sensitive for messages and sections.
- The retention bound prunes oldest-first.

### Integration tests

- One row per attempt; two rows for a retried attempt.
- A failed attempt retains a body; a successful attempt does not unless capture
  is enabled.
- The diagnostic reports digests and no body.

### Restart and recovery tests

- Rows survive a store reopen.
- A `streaming` outcome is reported as interrupted after a reopen.
- The migration runs from the previous version and is idempotent on re-run.

### Contention and cancellation tests

- Two concurrent sessions write without a busy error.
- Cancellation before dispatch writes no row; after dispatch writes
  `cancelled`.

### Security and negative tests

- A secret-shaped tool argument does not appear in a stored body.
- The diagnostic output contains no body content.
- A body is absent when capture is off and the attempt succeeded.

### Migration and compatibility tests

- Migration from the previous layout version, and the layout-version guard.

## 11. Required verification commands

```bash
# the domain and the migration
cargo test -p codegg-core
python3 scripts/check_project_catalog_invariants.py

# the seam and the reader
cargo test -p codegg --lib -- agent::provider_turn
cargo test -p codegg --lib -- session::request_ledger
cargo test --test agent_loop -- request_ledger

# the guard
python3 scripts/check_request_ledger_writers.py
python3 scripts/check_request_ledger_writers.py --self-test

# formatting and linting
cargo fmt --check
cargo clippy -p codegg --lib --tests -- -D warnings

# the routine set
scripts/verify.sh quick
```

The closure record states which commands ran and which did not.

## 12. Documentation updates

- `architecture/session.md` — the table, the single writer, the retention bound,
  the digest-versus-body rule, and the `streaming`-outcome meaning.
- `architecture/agent.md` — the attempt seam's new responsibility and its
  non-fatal failure rule.
- `architecture/observability.md` or `architecture/audit.md` if either owns the
  `doctor` subsystem list; otherwise the `doctor` module's own documentation.
- `plans/subsystems/harness-practice-adoption-roadmap.md` §12 — status only.

## 13. Acceptance criteria

1. A completed turn has exactly one ledger row per attempt, readable after a
   daemon restart.
2. A retried turn has two rows with distinct `attempt_index` values.
3. The stored `tools_digest` equals the value `tool_definitions_hash` produces
   for the same registry, proven by a test.
4. Changing one byte of a prompt section changes `system_digest` and changes
   only that section's entry in `section_digests`.
5. A successful attempt with capture disabled stores no body; a failed attempt
   stores a redacted body.
6. `codegg doctor`'s session subsystem prints the last row's digests and no
   body content.
7. The writer guard rejects all three of its self-test injections.
8. The request sent to the provider is byte-identical with the ledger enabled
   and disabled, proven by comparing the two requests in one test.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- the record cannot be expressed as an additive session-store row — a new
  `CoreRequest` variant or a field on the canonical session projection is an
  ADR, not a convenience;
- the digest cannot be computed from the dispatched `ChatRequest` without
  duplicating the prompt assembly;
- the reader has no production consumer that review accepts; a reader used only
  by tests is dead code, and dead code with a table behind it is worse than no
  table;
- the write cannot be made non-fatal on store failure without changing the
  turn's error semantics;
- the retention bound cannot be enforced inside the single writer without a
  second writer.

## 15. Closure evidence required

- The migration and the layout-version bump, with
  `check_project_catalog_invariants.py` green.
- The measured insert cost of the write, stated as a measurement rather than an
  assumption.
- The byte-identical-request test's output.
- The retry, cancellation, restart and contention test results.
- The diagnostic's output for a seeded store, showing digests and no body.
- The writer guard's `--self-test` output with all three injections rejected.
- The named non-claims: no backfill for pre-migration turns; a `streaming`
  outcome means the process died; the digest cannot be recomputed from the
  stored body.

## 16. Handoff notes

- **The write is non-fatal.** If the store is unavailable, the turn proceeds.
  Do not turn a ledger failure into a turn failure; the record serves the
  request, not the other way round.
- **`begin_immediate` is not optional.** The writer reads a count before it
  inserts and prunes. The queue path's guard exists because of exactly this
  shape.
- **Reuse `tool_definitions_hash`.** A second tool fingerprint will disagree
  with the first one eventually, and then nobody will know which is right.
- **Do not store a body by default.** The question "what did the model see?"
  is answered by the digests for the common case; a body is for the failure
  case.
- **Redaction happens at write, and the digest is over the unredacted value.**
  This is the one asymmetry in the design. Record it in the closure rather than
  leaving a reader to discover it.
- **The diagnostic prints digests.** Never a body. A body can contain a
  customer's source.
