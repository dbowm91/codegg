# Harness Practice Adoption Milestone 002 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/harness-practice-adoption/002-durable-request-ledger.md`

Source subsystem roadmap:

- `plans/subsystems/harness-practice-adoption-roadmap.md#m002--durable-request-ledger`

Repository baseline reviewed: `7642b9e38` (`feat-auto-20261109-f8abad0d`), plus
uncommitted working-tree work

Implementation commits or pull requests:

- **None.** This milestone exists only as uncommitted working-tree work on
  `feat-auto-20261109-f8abad0d`, on top of HEAD `7642b9e38`. `git status
  --porcelain` lists `crates/codegg-core/src/session/schema.rs`,
  `crates/codegg-core/src/storage/mod.rs`, `src/agent/provider_turn.rs`,
  `src/agent/loop.rs`, `src/agent/turn_runtime.rs`, `src/agent/worker.rs`,
  `src/main.rs` and `architecture/session.md` as modified;
  `crates/codegg-core/src/session/request_ledger.rs`,
  `scripts/check_request_ledger_writers.py` and
  `docs/storage-table-owners.toml`'s v95 entry are untracked additions. Nothing
  below is attributable to a SHA.

The source plan and the roadmap's §12 status table have since been updated by
the parent: both now read `conditionally closed` for M002
(`plans/implementation/harness-practice-adoption/002-durable-request-ledger.md:3`,
`plans/subsystems/harness-practice-adoption-roadmap.md:501`). This record states
the evidence; it does not edit either.

## 1. Executive finding

The capability is delivered and it answers the question it was written to
answer: "what did the model see in this turn?" is now read from storage rather
than reconstructed from today's assets. `turn_request_ledger` is created by
`migrate_v95` with `STORAGE_LAYOUT_VERSION = 95`, `record_attempt` is its single
writer, the write happens **before** `provider.stream` so an attempt that dies
at setup still has a row, retention is bounded to 256 rows per session and
pruned oldest-first inside the same `BEGIN IMMEDIATE`, bodies are redacted at
write and kept only for failures, and `codegg doctor session` is a production
consumer that prints digests and never a body.

Three limits are recorded rather than glossed:

1. **The insert cost is a measurement, not a guarantee.** `architecture/session.md`
   records 157.7 µs per attempt (mean of 20 inserts, in-memory migrated SQLite,
   debug build). A later re-measure of **168.31 µs** is reported by the
   implementing session but **is not recorded anywhere in the tree** — I
   searched `*.md`, `*.rs` and `*.toml` for it and found nothing. The only
   in-tree measurement is the doc's 157.7 µs.
2. **The byte-identical acceptance criterion is met by a narrower substitute.**
   The test compares a turn's *rendered output and event sequence* between a
   control loop with a working ledger and a subject loop whose ledger table was
   dropped; it does not compare the dispatched `ChatRequest` with the ledger
   enabled against one with it disabled.
3. **The section-digest wiring is tested at the seam, not at its production
   call sites.** The three tests assert that a loop carrying a section map
   writes it into the row; they call `set_prompt_section_digests` themselves, so
   deleting the production call would leave them green.

The one thing that changed since the plan was written is that the CLI gap the
plan inherited is **closed in this tree, by follow-up work, not by this
milestone** — see §3.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| `migrate_v95` creates `turn_request_ledger` and its index | `crates/codegg-core/src/session/schema.rs:539-570` (`CREATE TABLE IF NOT EXISTS turn_request_ledger`, `CREATE INDEX … idx_turn_request_ledger_session_turn`), wired at `:420` (`95 => migrate_v95(…)`) | pass | Additive: creates a table and an index, alters nothing |
| `STORAGE_LAYOUT_VERSION` tracks the highest migration | `crates/codegg-core/src/storage/mod.rs:61` = `95`; `check_project_catalog_invariants.py` "storage layout marker tracks highest wired schema migration" | pass | Guard run: 8/8 checks |
| Unique over `(session_id, turn_index, attempt_index)` | schema `UNIQUE`; test `a_second_row_for_one_attempt_is_refused` | pass | A duplicate attempt is impossible, not merely unlikely |
| Exactly one production writer | `check_request_ledger_writers.py` "one writer, ordered before dispatch, bounded, and read by doctor" | pass | Guard green; `--self-test` rejects an injected second writer |
| The writer reads before it writes, so it takes the write lock at `BEGIN` | `crates/codegg-core/src/session/request_ledger.rs:207` `begin_immediate(&self.pool)`; `check_queue_write_transaction_shape.py` | pass | Both guards green; the queue-shape guard reports the shared helper issues `BEGIN IMMEDIATE` |
| Retention bounded and enforced by the writer | `MAX_REQUEST_LEDGER_ROWS_PER_SESSION: i64 = 256` at `request_ledger.rs:25`; tests `retention_holds_the_bound_and_prunes_the_oldest_attempts`, `the_table_bound_stops_one_shot_sessions_from_growing_the_ledger` | pass | Prune is inside the same transaction |
| Digests computed from the dispatched request | `provider_turn.rs:536-541`: `system_prompt_digest(request)`, `tool_definitions_hash(request.tools…)`, `message_range_digest(&request.messages)` | pass | Not a rebuild; the seam has no prompt compiler |
| `tools_digest` equals `tool_definitions_hash` for the same registry | `provider_turn.rs:1086` `the_stored_tools_digest_is_the_registry_hash` | pass | Criterion 3 |
| One byte of a prompt section moves only that section's digest | `src/context/tool_hash.rs:328` `the_system_digest_is_stable_and_moves_on_one_byte`, `:414` `the_section_digest_separates_kind_and_content` | pass | Criterion 4 |
| Body redacted at write; kept only for a failure | `request_ledger.rs:360` `redact_transcript_body`; test `settling_stores_a_redacted_failure_body_and_none_for_success` | pass | Bounded by `REQUEST_LEDGER_MAX_BODY_BYTES` |
| `codegg doctor session` prints digests and no body | `src/main.rs:3464` `session_ledger_report_lines`; test at `:1329-1351` seeds `SECRET-MARKER-9f3` and asserts it is absent | pass | Criterion 6 |
| No ledger rows reports "no ledger rows" | `main.rs:3437`, `:3448` (both the missing-file and missing-table arms) | pass | An older store degrades, it does not fail |
| Writer guard rejects its self-test injections | `check_request_ledger_writers.py --self-test`: unmodified tree passes, second writer rejected, missing bound rejected, test-only reader rejected | pass | Criterion 7 — three injections plus a control |
| Rows survive a reopen; `streaming` reports as interrupted | `a_reopened_store_reads_the_rows_and_reports_streaming_as_interrupted` | pass | |
| Migration from v94 is idempotent | `session::schema::migration_upgrade_tests::v94_database_gains_the_request_ledger_and_reapplies_idempotently` | pass | Ran under the `request_ledger` filter |
| Two concurrent sessions write without a busy error | `two_sessions_write_concurrently_without_a_busy_error` | pass | |
| Request identity unchanged by the ledger | `a_ledger_store_failure_leaves_the_turn_unchanged` (`src/main.rs:5609-5660`) | **partial** | Compares turn output and event sequence, not `ChatRequest` bytes — see §4 |
| Section digests threaded from the compile site | `turn_runtime.rs:1117`, `worker.rs:2144`, `main.rs:4019` (single-shot) | **partial** | Three production call sites exist; no test fails if any is deleted — see §5 |
| Insert cost measured, not assumed | `architecture/session.md:474` records 157.7 µs; test `the_ledger_insert_cost_is_bounded` prints the number and asserts `< 50 ms` | **partial** | The 168.31 µs re-measure is not in the tree — see §4 |

## 3. Production implementation evidence

**Storage.** `migrate_v95` (`schema.rs:539`) creates `turn_request_ledger` with
the columns the plan names — `id`, `session_id`, `turn_index`, `attempt_index`,
`attempt_id`, `model`, `effort`, `system_digest`, `tools_digest`,
`message_digest`, `message_count`, `section_digests`, `outcome`, `body`,
`created_at_ms` — the `UNIQUE` constraint over
`(session_id, turn_index, attempt_index)`, and
`idx_turn_request_ledger_session_turn`. `STORAGE_LAYOUT_VERSION` is `95`
(`storage/mod.rs:61`). The migration is additive: it creates and alters
nothing, so a rollback is "drop the table and step the constant back", which is
not automated.

**The writer.** `TurnRequestLedgerStore::record_attempt`
(`request_ledger.rs:191`) is the single writer. It opens `begin_immediate`,
counts the session's rows, prunes the oldest beyond 256, and inserts — one
transaction. `settle_attempt` only `UPDATE`s the row the writer created, keyed
on the attempt identity, so a settled attempt can never become a second row.
The attempt-seam caller is `ledger_open_attempt` in `src/agent/provider_turn.rs`
(line 533 onward), which is reached before the stream is consumed.

**The digests.** Three producers, all in `src/context/tool_hash.rs`:
`system_prompt_digest`, `tool_definitions_hash` (reused, not duplicated — the
plan's §16 asked for exactly this), `message_range_digest`. Each has a stated
canonical form and a stability test. `section_digests` is the compiled prompt's
per-section map, produced by `crate::agent::prompt::section_digests(&blocks)` at
the compile site and carried on `AgentLoop` (`loop.rs:242`, setter at `:716`,
getter at `:723`). The seam reads it and never recompiles.

**Threading sites.** Three production call sites set the map:
`src/agent/turn_runtime.rs:1117` (the daemon turn path),
`src/agent/worker.rs:2144` (the delegated child) and `src/main.rs:4019` (the
CLI single-shot path). Plus `main.rs:5470` and `:5577`, which are tests.

**The production consumer.** `print_session_ledger_report` (`main.rs:3437`) is
reached from `DoctorSubsystem::Session`. It prints the retained row count, the
last attempt's identity and outcome, its model/effort selection, its three
digests and message count, and whether a body is present — never the body.

**The CLI gap, corrected.** The briefing for this record states that the CLI
single-shot path "writes no row at all because `run_single_shot` builds its loop
with `pool: None`". **That is no longer what this tree does.** `run_single_shot`
(`src/main.rs:3844`) now builds a `ledger_pool` from
`storage::init_migrated_daemon_catalog` (`main.rs:3925-3936`), warns and degrades
to `None` if the store cannot be opened, and passes it as `AgentLoop::new`'s
seventh argument (`main.rs:3943`). `git show HEAD:src/main.rs` has neither the
`ledger_pool` block nor the `RecordingProvider` test double, so this is
**uncommitted follow-up work in the same working tree**, separate from this
milestone. The gap as it stood at M002 was real; it is closed here by later
work, and this record does not claim it as M002's evidence.

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg-core --lib request_ledger
python3 scripts/check_request_ledger_writers.py
python3 scripts/check_request_ledger_writers.py --self-test
python3 scripts/check_queue_write_transaction_shape.py
python3 scripts/check_project_catalog_invariants.py
python3 scripts/check_comment_budget.py
git show HEAD:src/main.rs | grep -c RecordingProvider
```

### Results

- `cargo test -p codegg-core --lib request_ledger` — **11 passed, 0 failed**,
  1005 filtered out, 2.10 s. The eleven are the ten
  `session::request_ledger::tests::*` bodies plus
  `session::schema::migration_upgrade_tests::v94_database_gains_the_request_ledger_and_reapplies_idempotently`.
  The build reported `Blocking waiting for file lock on build directory` and
  finished in 4 m 35 s, because sibling agents were building concurrently.
- `check_request_ledger_writers.py` — green: "one writer, ordered before
  dispatch, bounded, and read by doctor".
- `check_request_ledger_writers.py --self-test` — green: the unmodified tree
  passes; an injected second writer is rejected; an injected missing bound is
  rejected; an injected test-only reader is rejected.
- `check_queue_write_transaction_shape.py` — green, three checks: queue write
  paths take the write lock at `BEGIN`; the shared helper issues `BEGIN
  IMMEDIATE` and rolls back safely; the scratch sweep decides each candidate
  under the writer lock.
- `check_project_catalog_invariants.py` — 8/8 checks pass, including "storage
  layout marker tracks highest wired schema migration" and "storage catalog
  exists and declares the layout version".
- `check_comment_budget.py` — **green in the 2026-11-13 `scripts/verify.sh full`
  run**, and a standalone re-run shows no violation in this milestone's file.
  That violation is discharged by a deliberate bank:
  `crates/codegg-core/src/session/request_ledger.rs` is recorded at 40 comment
  lines / 0.0599 (`docs/comment-budget.toml:101`); the corpus total is banked at
  139,245 lines. The 44-violation figure this bullet recorded is superseded. (At
  correction time the guard's remaining red traces to WP D's in-flight
  `diff.rs`/`baseline.rs`, including the corpus totals — not to this
  milestone's files.)
- `git show HEAD:src/main.rs | grep -c RecordingProvider` — `0`, which is how
  the CLI follow-up is distinguished from committed history.

### Not run

- `cargo test -p codegg --lib -- session::request_ledger` from the plan's §11:
  the module lives in `codegg-core`, not the root package, so the equivalent
  command is the one above.
- `cargo test --test agent_loop -- request_ledger`: **not run.** The plan lists
  it; the ledger's loop-level coverage was instead read at
  `provider_turn.rs:1086-1330`, which exercises the seam directly.
- `cargo fmt --check` and `cargo clippy -D warnings`: **not run in the authoring
  session** — the tree did not compile for the root package's lib-test target
  then because of a sibling lane's in-flight edit (§6). **Superseded 2026-11-13:**
  `scripts/verify.sh full` ran on this tree and passed, with `cargo fmt --all
  --check` clean and clippy `-D warnings` clean.
- `scripts/verify.sh quick`: **not run in the authoring session**;
  **superseded 2026-11-13** by the green `scripts/verify.sh full` run, which
  includes `quick` and every guard.
- The insert-cost test was **not re-run** here. The 157.7 µs figure is the
  doc's recorded measurement, and the 168.31 µs figure that the briefing cites
  has no in-tree artefact.

## 5. Invariant review

- **The ledger does not change what is sent.** The writer only reads the
  `ChatRequest`; it binds digests and the model/effort strings. Evidence:
  `a_ledger_store_failure_leaves_the_turn_unchanged` holds turn output and
  event sequence equal between a working-ledger control and a dropped-table
  subject, and asserts the control recorded a row so the comparison is not two
  silent no-ops. **Limit:** this is output equivalence, not byte comparison of
  the two `ChatRequest` values; the plan's criterion 8 asked for the latter.
- **One writer.** Guard-enforced, and the guard's `--self-test` injects a
  fourth caller to prove the rule is not vacuous.
- **The digest is over the dispatched value.** `ledger_open_attempt` takes the
  same `request` reference it hands to the provider; there is no second
  assembly path.
- **The digest is not over redacted content.** `architecture/session.md:464-468`
  states the asymmetry and that a reader cannot recompute the digest from the
  body. The code matches: digests are computed before the body is redacted.
- **No secret material is stored.** Redaction at write, plus the doctor test
  that seeds `SECRET-MARKER-9f3` and asserts its absence from the rendered
  report.
- **Retention is bounded and enforced by the writer.** Constant `256`; two
  tests; the prune is inside the writer's transaction.
- **The migration is restart-safe and additive.** Tested from v94 and re-applied
  idempotently.
- **Section digests come from the compile site.** The three production call
  sites exist, but **no test fails if one is deleted**: every test that asserts
  a non-`NULL` `section_digests` calls `set_prompt_section_digests` itself
  (`main.rs:5577-5578`, `worker.rs:3114`, and the pure setter round-trip at
  `main.rs:5440-5475`). Deleting `turn_runtime.rs:1117`, `worker.rs:2144` or
  `main.rs:4019` would leave all three green. The briefing's warning is
  correct, and I confirmed it by reading each test rather than copying the
  claim.

## 6. Failure and recovery review

- **Failure before dispatch** — no row: `record_attempt` is reached only from
  the attempt seam.
- **Failure at stream setup** — one row, outcome `failed`: the write precedes
  the stream.
- **Failure mid-stream** — one row, `failed`, body retained redacted.
- **Cancellation** — before dispatch no row; after dispatch the row settles
  `cancelled`. `TurnRequestOutcome` carries all three terminal values plus
  `streaming`.
- **Retry** — each attempt is its own row with the next `attempt_index`; the
  unique key makes a duplicate impossible. `one_row_per_attempt_and_the_reader_orders_by_attempt`.
- **Restart** — rows are durable; a row still `streaming` on read is reported as
  interrupted (`a_reopened_store_reads_the_rows_and_reports_streaming_as_interrupted`).
  This is the honest record of a process that died.
- **Contention** — `begin_immediate` makes one caller wait rather than fail;
  `two_sessions_write_concurrently_without_a_busy_error`.
- **Store unavailable** — non-fatal by design. `open_ledger_turn` returns
  `None` and logs. Two tests cover it: `a_ledger_write_failure_leaves_the_request_unrecorded_but_unharmed`
  (`provider_turn.rs:1163`) and, end to end through the CLI loop,
  `a_ledger_store_failure_leaves_the_turn_unchanged` (`main.rs:5609`). **Neither
  ran under this record's filter** — neither name contains `request_ledger`, and
  the second lives in the bin target — so both are read, not executed.
- **Bounded body** — `REQUEST_LEDGER_MAX_BODY_BYTES` with a char-boundary-safe
  truncation (`request_ledger.rs:360-372`).

## 7. Migration and compatibility review

- `migrate_v95` is additive and is the highest wired step; the layout constant
  and the highest migration agree, guard-enforced.
- An older daemon reading a `v95` store is refused by the existing layout check;
  no new negotiation was introduced.
- A store without the table degrades to "no ledger rows" in the doctor path
  (`is_missing_table` arm), so the diagnostic is safe against a pre-v95 store.
- **No backfill.** Turns that predate the migration have no rows, and the reader
  reports that rather than inventing them.
- No protocol, configuration or dependency change. The plan's stop condition —
  a record that cannot be expressed as an additive row — was not hit.

## 8. Security review

- Authorization is unchanged: the doctor path uses the same catalog access
  `doctor` already had and adds no capability; the writer is daemon-internal.
- Redaction happens at write, before the row exists, so a body cannot be read
  back unredacted. The doctor test proves body text never reaches the report.
- The digest is over the whole request and is not per field, so it creates no
  per-field oracle over a short secret. Stated in the plan; the code's single
  digest-per-dimension shape matches.
- Bounds: 256 rows per session, a body byte cap, and a read ordered by
  `(turn_index, attempt_index)`.
- `section_digests` is stored as JSON and parsed back with a typed error rather
  than `unwrap` (`record_from_row`, `request_ledger.rs:374-380`), so a malformed
  row fails the read instead of a panic.

## 9. Documentation and operations

- `architecture/session.md:440-476` — the table, the single writer, the
  `BEGIN IMMEDIATE` reason, the retention bound, the digest-versus-body rule,
  the `streaming` meaning, and the measured insert cost.
- `architecture/agent.md` — modified in the working tree; the attempt seam's
  responsibility and its non-fatal failure rule are the reported changes.
- `architecture/storage-catalog.md` — contains the generated row for
  `turn_request_ledger` at layout 95 (M006's generator).
- `scripts/check_request_ledger_writers.py` — wired into `scripts/verify.sh`
  (both the `--self-test` and the plain run) and into `.github/workflows/ci.yml`.
- Operator recovery: `codegg doctor session`. A `streaming` outcome is the
  signal that a previous process died mid-attempt.

## 10. Unresolved findings

The comment-budget and roadmap/plan findings below were resolved on 2026-11-13
and are retained so the conditions are not silently dropped. The three
substantive findings above them remain open; the 2026-11-13 `full` run does not
settle any of the three (it re-runs the existing tests, adds no per-call-site
test, no `ChatRequest` comparison and no new measurement).

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | The section-digest threading is not covered at its three production call sites; every test sets the map itself, so deleting `turn_runtime.rs:1117`, `worker.rs:2144` or `main.rs:4019` leaves all tests green | A regression that silently stops recording section identity in the daemon path would pass the suite | Add one test per call site that drives the real turn and asserts the row's `section_digests` is `Some`, without the test calling the setter |
| low | Acceptance criterion 8 is met by output equivalence, not `ChatRequest` byte comparison | A change that altered the request only when the ledger is enabled would not be caught | Add a test that captures the dispatched `ChatRequest` under both conditions and compares it |
| low | The 168.31 µs re-measure is not recorded in the tree; only 157.7 µs is | Two numbers circulate with no artefact for the second | Either record the second measurement with its host and build profile, or drop it |
| resolved (was low) | `crates/codegg-core/src/session/request_ledger.rs` **was** absent from `docs/comment-budget.toml` while holding 40 comment lines | `check_comment_budget.py` was red partly because of this file | Banked at 40 comment lines / 0.0599 (`docs/comment-budget.toml:101`); guard green 2026-11-13 |
| resolved (was low) | The roadmap §12 table and the plan's own `Status:` line said `not started` / `ready for handoff` | A reader of the roadmap saw no progress on delivered work | Parent updated both: the roadmap row now reads `conditionally closed` (`plans/subsystems/harness-practice-adoption-roadmap.md:501`) and the plan's line 3 matches |

**Owner ruling, 2026-11-12 — the CLI session row, accepted as built.** The
request ledger's CLI path creates an **ephemeral session row** rather than
writing ledger rows that reference a session id with no row (which failed the
usage foreign key). The owner has accepted both consequences rather than
reopening the design: (i) the CLI's session id is opaque —
`cli-<sha256(canonical root)>` with the worktree path carried in
`project.worktree` — so one-shot runs do **not** appear in project-scoped
listings and therefore cannot become the TUI's "most recent session"; and (ii)
a CLI run's transcript is **durable** (two message rows per run) and the
session row is never deleted. Both are reversible; neither is considered a
defect. The per-session bound of 256 ledger rows never fires for a one-shot id,
so the table-wide cap `MAX_REQUEST_LEDGER_ROWS_TOTAL = 256 × 32` is what bounds
this path, inside the same `BEGIN IMMEDIATE` as the write.

## 11. Roadmap disposition

**Conditionally closed.** The capability, its migration, its single writer, its
bound, its redaction rule and its production consumer are implemented and the
required tests ran here (11 passed). The conditions are named, not implied:

1. **Re-measure the insert cost on a named host** and record the number with
   its build profile in `architecture/session.md`. Exact command:
   `cargo test -p codegg --lib the_ledger_insert_cost_is_bounded -- --nocapture`.
   The test prints `request ledger insert cost: <duration> per attempt`.
2. **Close the call-site coverage gap** in finding 1 of §10 with one test per
   site.
3. **Resolve the byte-identity criterion** by adding the two-request comparison
   or by amending the plan's criterion 8 to the narrower claim the test proves.

None of the three blocks the next milestone: M002's consumers (the doctor
diagnostic and M001's comparison) work without them. The 2026-11-13 green
`scripts/verify.sh full` does not discharge them either: it re-ran the existing
suite, and none of the three has a test or an artefact that run could satisfy.

## 12. Registry updates

Required in `plans/registry.md` and
`plans/subsystems/harness-practice-adoption-roadmap.md`:

- §12's M002 row: `not started` → `conditionally closed`, closure record
  `plans/closure/harness-practice-adoption/002-status.md`, blocker column naming
  the insert-cost re-measurement, the call-site coverage and the byte-identity
  criterion. **Done by the parent:** the roadmap row now reads `conditionally
  closed` (`plans/subsystems/harness-practice-adoption-roadmap.md:501`).
- The registry's milestone table entry for M002, if `plans/registry.md` carries
  one.
- **Not updated by this record:** the source plan's `Status:` line; the parent
  has since changed it to `conditionally closed`
  (`plans/implementation/harness-practice-adoption/002-durable-request-ledger.md:3`).

## Correction log (2026-11-13)

The gate-class discharge cited below is the 2026-11-13 `scripts/verify.sh full`
run on this tree: 17,377 workspace tests and 8,441 feature-pass tests, 0
failures; `cargo fmt --all --check` clean; clippy `-D warnings` clean. `full`
includes `quick`, which includes the guards.

- **Preamble** — "the source plan still reads `ready for handoff`, and the
  roadmap … still reads `not started`": superseded by the parent's updates; both
  now read `conditionally closed` (plan:3, roadmap:501).
- **§4 results, comment budget** — "red, exit 1, 44 violations": the milestone's
  file is banked at 40 lines / 0.0599 (`docs/comment-budget.toml:101`), the
  corpus is banked at 139,245 lines, and the guard was green in the 2026-11-13
  `full` run.
- **§4 "Not run" (fmt, clippy)** — superseded by the 2026-11-13 `full` run,
  which ran both clean.
- **§4 "Not run" (`quick`)** — superseded by the same run; `full` includes
  `quick`.
- **§10 comment-budget row** — resolved by the recorded bank, retained above.
- **§10 roadmap/plan row** — parent updated both, retained above.
- **§12 registry bullets** — corrected to name all three open conditions and to
  record the parent's update.

**Residue kept open, deliberately:** the insert-cost re-measurement (the 168.31 µs
figure still has no in-tree artefact; only `architecture/session.md:474`'s
157.7 µs exists), the per-call-site section-digest coverage, and the
`ChatRequest` byte-identity comparison. None is a gate condition and none is
discharged by the 2026-11-13 run.
