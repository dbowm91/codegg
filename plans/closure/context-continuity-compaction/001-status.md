# Context Continuity and Compaction M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/context-continuity-compaction/001-durable-continuation-checkpoint-and-epoch-foundation.md`

Source subsystem roadmap:

- `plans/subsystems/context-continuity-compaction-roadmap.md#8-ordered-milestones`

Repository baseline reviewed: `8e324f05`

Implementation commits or pull requests:

- `fde6c2e3` — context-continuity M001: durable continuation checkpoint and epoch foundation

## 1. Executive finding

M001 is complete. The durable host-owned foundation for safe multi-window
continuation landed: a typed continuation-checkpoint model with explicit
lineage and lifecycle, a bounded append-oriented SQLite store, restart-safe
`Prepared | Installed | Aborted` state, atomic installation of a checkpoint
with its durable `ContextCompacted` commit-marker event, typed load/query
APIs for later milestones, compatibility-safe optional lineage fields on
`ContextCompactedEvent`, and storage/documentation updates. No
model-visible compaction strategy, default, retained-message behavior, or
model-facing rendering changed in this milestone.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Typed continuation-checkpoint model with lineage/lifecycle (§6.1) | `crates/codegg-core/src/session/continuation.rs`: `ContinuationCheckpointStatus`, `ContinuationCheckpoint`, `ContinuationCheckpointPayload` + 13 unit tests | pass | UUID identity, payload schema v1 independent of storage layout, explicit parent/sequence semantics, typed constructor/size/digest checks |
| Bounded append-oriented SQLite store (§6.2) | `migrate_v57` + `STORAGE_LAYOUT_VERSION` 56 → 57; `continuation_checkpoint` table + 3 indexes + `UNIQUE(session_id, sequence)` | pass | Dedicated table; historical `checkpoints` table and goal Markdown journal untouched |
| Prepared/Installed/Aborted lifecycle (§6.3) | `ContinuationCheckpointStore::{prepare, get, latest_installed, mark_aborted, install_with_compaction_event}` + transition matrix test | pass | `latest_installed` never returns `Prepared`/`Aborted`; stale-parent install rejected; install of `Aborted` refused; abort of `Installed` refused |
| Lineage and digest verification (§6.3) | SHA-256 over canonical envelope JSON; `verify_digest()` on every read; parent precondition checked inside prepare and install transactions | pass | Tamper test proves forged `payload_json` fails with digest mismatch |
| Atomic install + durable event (§6.3) | `install_with_compaction_event` single-transaction update + `EventStore::append_in_tx`; `install_and_event_commit_atomically` test | pass | Stable event ID `continuation-checkpoint:<checkpoint_id>`; retry converges; conflicting event fails closed |
| Reused event serialization/conflict logic (§6.3) | `EventStore::append_in_tx` sharing serialization + `events_semantically_equal` with `append_idempotent` | pass | No second event-row encoding path; `ContextCompacted::semantic_equals` ignores only `created_at` |
| Old `ContextCompactedEvent` JSON readable (§6.4) | `#[serde(default)]` optional `checkpoint_id`, `checkpoint_digest`, `epoch_sequence`, `previous_checkpoint_id`, `continuity_degraded_reason` + `old_context_compacted_json_remains_deserializable` test | pass | `TuiSessionState` derivation unchanged |
| Small application-facing helper (§6.4) | `ContinuationCheckpoint::build_compacted_event` | pass | Later `AgentLoop` integration needs no manual SQL/event serialization |
| No frontend protocol change (§6.5) | No `CoreEvent`/ACP/projection/HTTP change | pass | Checkpoint payloads never exposed through projections |
| Runtime behavior unchanged (§6.6) | No change to `compact_if_needed` / `compact_context` / strategy selection | pass | `tests/compaction.rs` 65/65 pass unmodified |
| Hidden-reasoning/secret exclusion (§6.7) | Payload constructor denylist (reasoning + credential classes, recursive) + `hidden_reasoning_rejected`, `nested_secret_tool_argument_rejected`, `secret_fixture_not_copied_by_envelope_serialization` tests | pass | Diagnostics carry IDs/digests/sizes only |
| Payload/diagnostic bounds (§6.2, §8) | 128 KiB payload envelope, 1024-char diagnostics, 128-char IDs, 64×512-char event metadata; enforced before insert + SQLite `CHECK` defense-in-depth | pass | Oversized test proves rejection before row insertion |
| Focused migration/restart/contention tests (§10) | `crates/codegg-core/tests/continuation_checkpoint.rs` 11 tests | pass | Migration, prepare/get/latest, atomicity, idempotency, contention, abort, restart, tamper, bounds, diagnostics |
| Architecture docs and exports (§6.8, §12) | `architecture/session.md`, `compaction.md`, `context-compaction-ownership.md`, `storage.md`; `session::continuation` exports | pass | Ownership, lifecycle, restart semantics, bounds, durable-event role documented |
| No new CI lane (§11) | None added | pass | Narrowed clippy target documented below instead of the plan's `--all-features` workspace sweep, per repo no-`--all-features` policy |

## 3. Production implementation evidence

Ownership preserved: `codegg-core` owns durable session-scoped
continuation records; `src/context/compaction.rs` remains the sole
compaction-policy owner; the store performs no provider/model calls.

Final table/index schema (migration v57, layout 57):

```sql
CREATE TABLE IF NOT EXISTS continuation_checkpoint (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    sequence INTEGER NOT NULL CHECK (sequence > 0),
    previous_installed_id TEXT,
    schema_version INTEGER NOT NULL CHECK (schema_version > 0),
    status TEXT NOT NULL CHECK (status IN ('prepared', 'installed', 'aborted')),
    payload_digest TEXT NOT NULL,
    payload_json TEXT NOT NULL CHECK (length(payload_json) <= 131072),
    abort_reason TEXT CHECK (abort_reason IS NULL OR length(abort_reason) <= 1024),
    created_at INTEGER NOT NULL,
    installed_at INTEGER,
    aborted_at INTEGER,
    FOREIGN KEY (session_id) REFERENCES session(id) ON DELETE CASCADE,
    UNIQUE(session_id, sequence)
);
CREATE INDEX IF NOT EXISTS idx_continuation_checkpoint_latest
    ON continuation_checkpoint(session_id, status, sequence DESC);
CREATE INDEX IF NOT EXISTS idx_continuation_checkpoint_session_id
    ON continuation_checkpoint(session_id, id);
CREATE INDEX IF NOT EXISTS idx_continuation_checkpoint_lineage
    ON continuation_checkpoint(session_id, sequence);
```

Lifecycle transition table (enforced by `can_transition_to` plus store
preconditions):

| From → To | Allowed | Enforced by |
|---|---|---|
| Prepared → Installed | yes | `install_with_compaction_event` + parent/digest revalidation in-tx |
| Prepared → Aborted | yes | `mark_aborted` |
| Aborted → Aborted | yes (idempotent) | `mark_aborted` returns existing row |
| Installed → Installed | yes (idempotent retry only) | `install_with_compaction_event` + semantic event reconciliation |
| Prepared → Prepared | no | no API performs it |
| Installed → Aborted | no | `mark_aborted` rejects |
| Aborted → Installed | no | `install_with_compaction_event` rejects |
| Aborted → Prepared | no | payloads immutable after prepare |
| Installed → Prepared | no | payloads immutable after prepare |

`ContextCompactedEvent` gained five `#[serde(default)]` optional lineage
fields; the continuation payload itself is never embedded in the event.
`EventStore::append_idempotent` was refactored to share its collision
reconciliation (`events_semantically_equal`, covering
`ToolProgramNotification` and `ContextCompacted`) with the new
transaction-aware `EventStore::append_in_tx`.

No ADR was required: durable continuation authority stayed in
`codegg-core` session storage and no public cross-service storage
contract was introduced (additive table + additive optional event
fields only), per the plan's ADR threshold.

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg-core --lib session::continuation
cargo test -p codegg-core --test continuation_checkpoint
cargo test -p codegg-core --locked -- --test-threads=1
cargo test --test storage_migrations --locked -- --test-threads=1
cargo test --test compaction --locked -- --test-threads=1
python3 scripts/check_project_catalog_invariants.py --verbose
cargo fmt --all -- --check
cargo clippy -p codegg-core --all-targets --locked -- -D warnings
scripts/verify.sh quick
```

### Results

- `cargo test -p codegg-core --lib session::continuation`: 13/13 pass
  (status matrix, payload bounds, deterministic digest, tamper,
  redaction, diagnostics, ID validation, old-event compat, semantic
  equality).
- `cargo test -p codegg-core --test continuation_checkpoint`: 11/11
  pass (migration/layout v57, prepare/get/latest, lineage sequences,
  install+event atomicity, duplicate-install idempotency + conflicting
  collision, same-parent contention, abort semantics + candidate
  cleanup, file-backed restart recovery, tamper fail-closed, oversized
  + invalid-input rejection, diagnostics redaction).
- `cargo test -p codegg-core --locked -- --test-threads=1`: full crate
  pass — 646 lib tests + 6 + 11 + 2 + 18 integration tests, 0 failed.
- `cargo test --test storage_migrations`: 4/4 pass (existing
  migration suite unaffected; terminal version now 57 via the
  layout-tracking guard below).
- `cargo test --test compaction`: 65/65 pass (production compaction
  behavior unchanged).
- `python3 scripts/check_project_catalog_invariants.py --verbose`:
  7/7 pass, including layout-marker-tracks-highest-migration.
- `cargo fmt --all -- --check`: pass.
- `cargo clippy -p codegg-core --all-targets --locked -- -D warnings`:
  pass (one `match_like_matches_macro` finding fixed by using
  `matches!`). The plan's suggested workspace `--all-features` clippy
  sweep was deliberately not used: repo policy forbids
  `--all-features` for workspace sweeps (it drags in real-server
  tests); the narrowest affected crate plus `verify.sh quick`'s
  workspace `cargo check --all-targets` is the justified substitute.
- `scripts/verify.sh quick`: pass (fmt, builtin-agents check,
  core-boundary, sandbox, execution-ownership, tui-authority,
  workspace check).

## 5. Invariant review

| Plan invariant (§4) | Evidence |
|---|---|
| `codegg-core` as durable session-state owner | Store lives in `session::continuation`; `check-core-boundary.sh` passes |
| `src/context/compaction.rs` as compaction-policy owner | No compaction file touched except docs; compaction tests unmodified and green |
| No provider/model call from the checkpoint store | No provider import in `continuation.rs`; boundary guard passes |
| No hidden-reasoning persistence | Constructor denylist + tests; `Message::Reasoning` never referenced |
| No checkpoint body in logs or ordinary event payloads | `diagnostic_summary` helpers exclude bodies; event carries digest/IDs only; diagnostics test |
| Old `ContextCompactedEvent` JSON deserializable | `serde(default)` + round-trip test of pre-M001 JSON |
| Old databases migrate forward without rewriting history | Additive v57; restart-safe rerun test; goal/transcript tables untouched |
| Installation never inferred from prepared-row presence | `latest_installed` filters `status = 'installed'`; prepared-ignored test |
| `latest_installed` never returns Prepared/Aborted | Filter + tests for both states |
| Stale candidate cannot install over newer parent | Parent compared in-tx at prepare and install; contention test |
| Restart distinguishes abandoned preparation from committed epoch | File-backed restart test: installed recovered, prepared diagnostic-only, event count 1 |
| Payloads and diagnostics bounded before insert | Constructor + store + SQLite `CHECK` bounds; oversized test proves no row inserted |

## 6. Failure and recovery review

- Duplicate delivery/idempotency: install retry with semantically
  identical content converges (`duplicate_install_retry_converges_idempotently`);
  conflicting event for the same checkpoint identity fails closed with
  `session event identity collision`.
- Cancellation races: store operations are SQLx transactions; a crash
  before prepare commit leaves no row, after prepare leaves a
  non-resumable `Prepared` row, during install rolls back both state
  and event, after install exposes both. Covered by the atomicity and
  restart tests.
- Daemon restart: file-backed reopen test proves installed recovery,
  prepared ignored, event durable.
- Partial persistence failure: stale-parent prepare fails before any
  insert; stale-parent install fails inside the transaction with no
  event written (event count stays 1).
- Stale generation/lease: session-scoped lineage precondition in-tx;
  two same-parent contenders cannot both install.
- Contention/resource release: `UNIQUE(session_id, sequence)` fails
  closed on sequence races; `busy_timeout` + single-writer SQLite
  serializes writers under the existing transaction policy.
- Malformed/unauthorized input: empty/slashed/whitespace/control IDs
  rejected; oversized payloads/events rejected; tampered digests fail
  closed; FK to `session(id)` rejects orphan checkpoints.
- Bounded event/artifact behavior: event metadata capped at 64 items ×
  512 chars; payload capped at 128 KiB with DB-level `CHECK`.

## 7. Migration and compatibility review

- Additive migration v57; `STORAGE_LAYOUT_VERSION` 56 → 57 in lockstep;
  the dynamic guard (`check_project_catalog_invariants.py`) passes
  without edits, proving no second version guard was added.
- Existing sessions have no continuation rows and behave exactly as
  before; no backfill from summaries or goal Markdown was performed.
- Historical `checkpoints` table and goal journal remain valid and
  separate; migration test asserts all three coexist.
- `ContextCompactedEvent` extension is forward-compatible for old
  readers of old JSON and forward-only for new lineage fields.
- `TuiSessionState::from_events` ignores the new optional fields, so
  derived TUI state is unchanged.
- Rollback limitation: downgrading a v57 database to a v56 runtime
  leaves the additive table/indexes inert but present; no historical
  data is rewritten in either direction.

## 8. Security review

- Payload constructor rejects provider-hidden reasoning classes
  (`reasoning`, `hidden_reasoning`, `chain_of_thought` variants) and
  secret-bearing classes (`api_key`, `authorization`, `credential`,
  `secret`, `bearer`, `password`, `private_key`), recursively —
  tested at top level and nested inside tool-argument-shaped objects.
- Envelope serialization never scrapes transcripts: the body is an
  explicitly supplied JSON object; the secret-fixture test proves a
  secret-like tool argument is not copied by serialization.
- Store/log errors carry checkpoint/session IDs and digests only;
  `diagnostic_summary` tests assert payload bodies never appear.
- Identifiers are validated and all SQL remains parameterized; no
  filesystem path is used as checkpoint identity (UUIDs).
- Same-session enforcement is structural: every query scopes by
  `session_id`, the event session must match, and the FK binds rows to
  `session(id)` with cascade delete.
- No new network, auth, permission, or export surface was added;
  continuation records are not included in session exports.

## 9. Documentation and operations

Updated:

- `architecture/session.md` — continuation module in tree, v57 table
  group, event lineage + `append_in_tx` semantics,
  `ContinuationCheckpointStore` API/lifecycle, test targets, semantic
  equality gotcha.
- `architecture/compaction.md` — M001 durable-foundation section,
  commit-marker lineage, explicit no-behavior-change statement.
- `architecture/context-compaction-ownership.md` — store ownership row
  in the production-path inventory; corrected the stale "no
  compaction state is persisted separately" claim.
- `architecture/storage.md` — v57 migration entry.

Operator notes: stale `Prepared` rows may remain for diagnostics after
crashes; there is intentionally no background cleanup task in M001.
`delete_candidate` is the bounded helper for removing `Prepared` /
`Aborted` rows in tests and future retention work; it refuses to
delete `Installed` epochs.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| — | None | — | — |

No stop condition triggered (no external service needed, historical
table untouched, event extended compatibly, compaction policy stayed
out of `codegg-core`, contention resolved with existing SQLite
policy, no hidden-reasoning requirement arose).

## 11. Roadmap disposition

Milestone closed and next dependencies may proceed:

- M002 (authoritative intent, plan, and frame projection): hard
  dependency on M001 satisfied — unblock to `ready`.
- M003 (bounded exact context recovery references): hard dependency
  on M001 satisfied — unblock to `ready` (may run in parallel with
  M002).
- M004 remains `blocked` on M002 and M003 accepted closure.

## 12. Registry updates

- `plans/registry.md`: M001 `ready` → `closed` with closure link and
  implementation commit `fde6c2e3`; M002 and M003 `blocked` → `ready`;
  subsystem row current milestone M001 ready → M001 closed, M002/M003
  ready; execution-order item 1 rewritten to reflect the unblock;
  closure-work control row updated; M001 appended to recently-closed
  work.
- `plans/subsystems/context-continuity-compaction-roadmap.md`: status
  line, M001 section (`ready` → `closed` with closure link), M002/M003
  sections (`blocked` → `ready`), dependency-graph note.
- `plans/implementation/context-continuity-compaction/001-*.md`:
  `Status: ready` → `Status: implemented`.
- `plans/implementation/context-continuity-compaction/002-*.md` and
  `003-*.md`: `Status: blocked on M001 accepted closure` →
  `Status: ready`.
