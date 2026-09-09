# Identity, Authorization, and Audit Milestone 004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/identity-authorization-audit/004-append-only-audit-foundation.md`

Source subsystem roadmap:

- `plans/subsystems/identity-authorization-audit-roadmap.md#M004--append-only-audit-foundation`

Repository baseline reviewed: `07221c30` (`feat(identity): M004 append-only audit foundation, migration v54, authorized query/export`)

Implementation commits:

- `07221c30` — M004 append-only audit foundation: canonical audit event
  builder/store/writer, migration v54, bounded redacted metadata,
  retention separation, authorized bounded query/pagination/filter/export
  protocol plus daemon dispatch, audit guard, architecture docs,
  unit + boundary integration tests.

## 1. Executive finding

M004 is closed. The coordinator owns an append-only structural audit
record/store with typed attribution, bounded redacted metadata,
deterministic ordering/idempotency, authorized bounded
query/pagination/filter/export, and content-retention separation. An
authorized owner can query an ordered attributable structural log
across restart; bodies expire independently; duplicates never
duplicate events; secrets never reach storage or export;
failure/backpressure is bounded and observable. No unresolved high,
medium, or low M004 finding remains.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Schema, visibility/action taxonomy, bounded metadata and correlation fields (work package A) | `crates/codegg-core/src/audit.rs`: `AuditAction::ALL` (25 actions), `AuditVisibility` (project/session_participants/actor_only/administrators), metadata bounds (16 entries, 64/512/4096 bytes), correlation locators bounded to 128 bytes | pass | Writers parse actions strictly; readers parse leniently to `Unknown` so future actions degrade safely. |
| Transactional append/idempotency/sequence and restart-safe migration (work package B) | Migration v54 (`audit_event` AUTOINCREMENT seq + UNIQUE event_id, `audit_body`); `AuditStore::append` single-statement `INSERT ... ON CONFLICT(event_id) DO NOTHING`; `STORAGE_LAYOUT_VERSION` 53 → 54 | pass | Coordinator is the single sequence authority; `sqlite_sequence` continues across restart. |
| Redaction/content-digest/body-retention separation and secret-negative tests (work package C) | Key charset + secret key/value deny-lists, `audit_secret_detected` rejection before any write, SHA-256 metadata/content digests, `expire_bodies` deleting only `audit_body` rows | pass | Bodies that scan as credential text are rejected at build time; digests carry no secret material. |
| Authorized bounded query/filter/pagination/export (work package D) | `AuditQueryFilter` (limit clamp 1–100, cursor pagination, project/action/principal filters), `export` envelope with `export_digest`, protocol DTOs + `AuditCapabilities` negotiation, `audit_query`/`audit_export` as `DirectProject` + `audit.read` with daemon dispatch | pass | Unknown filters degrade to empty pages, never to existence-leaking errors. |
| Write failure/backpressure observability without fabricating success (work package E) | `AuditWriter` (semaphore-bounded inflight, write timeout, fail-fast `try_append`), `AuditWriterMetrics` (appended/duplicates/failed/dropped_backpressure/timeouts/queue_depth), typed `Backpressure`/`Timeout` errors | pass | Both failure policies report the error; neither invents an appended event. |
| Concurrent ordering test | `concurrent_append_assigns_unique_ordered_sequence` (16 concurrent appends, unique strictly increasing seq) | pass | `multi_thread` with `worker_threads = 2` per Tokio flavor rules; file not flagged by `audit_tokio_tests.py`. |
| Duplicate append test | `duplicate_event_id_is_idempotent` (unit + integration) | pass | Same seq returned, count stays 1. |
| Restart sequence test | `restart_continues_sequence_safely` (file-DB close/reopen, seq 1,2 → 3, ordered page) | pass | — |
| Migration test | `migration_is_additive_and_restart_safe` + `storage_migrations` (4 passed) | pass | Remigration safe; terminal version equals `STORAGE_LAYOUT_VERSION`. |
| Secret corpus negative test | `secret_corpus_never_reaches_storage_or_export` (7 keys, 7 values, 1 body; count 0, empty export) | pass | Sentinel-bearing values rejected before any write. |
| Metadata/body bounds tests | `metadata_and_body_bounds_are_enforced` (entry/value/body limits) | pass | Exact wire codes asserted. |
| Retention expiry preserving structure | `retention_expiry_preserves_structure` + `body_retention_expiry_preserves_structure` | pass | Body gone, structural row + digests intact. |
| Pagination/filter stability | `pagination_and_filter_stability` (cursor pages, action/principal filters) | pass | — |
| Unauthorized reads | `daemon_unauthorized_reads_are_denied` (Viewer + outsider denied on query and export with `authorization_denied`) | pass | Enforcement at the M003 gate; store reads are coordinator-internal. |
| Writer failure/backpressure | `writer_failure_and_backpressure_are_bounded_and_observable` + unit backpressure test | pass | Saturated `try_append` fails fast; metrics observable. |
| Export digest/integrity | `export_digest_covers_ordered_events` (stability, reorder sensitivity, empty digest) | pass | 64-hex SHA-256 over canonical ordering. |

## 3. Production implementation evidence

- `crates/codegg-core/src/audit.rs` (new, ~1770 lines with tests):
  `AuditAction`/`AuditVisibility` taxonomies, `AuditError` with stable
  wire codes, bounded metadata validation + secret deny-lists,
  SHA-256 digest helpers, `AuditDecisionProvenance` +
  `AuditEventBuilder` (transport-bound principal, no authority
  setters), `AuditEvent` + wire DTO conversion, `AuditQueryFilter` /
  `AuditPage` / `AuditExport` / `export_digest`, `AuditStore`
  (append/get/query/export/read_body/expire_bodies/max_seq/count),
  `AuditFailurePolicy` / `AuditWriterConfig` / `AuditWriterMetrics` /
  `AuditWriter`, `audit_capabilities_dto`, 14 focused tests.
- `crates/codegg-core/src/session/schema.rs`: additive `migrate_v54`
  (`audit_event` + `audit_body`, `IF NOT EXISTS` tables/indexes,
  visibility CHECK); wired into the version dispatcher (`53 → 54`).
- `crates/codegg-core/src/storage/mod.rs`: `STORAGE_LAYOUT_VERSION`
  53 → 54.
- `crates/codegg-core/src/lib.rs`: `pub mod audit;` (boundary-clean:
  only `team`, `transport_auth`, `error`, `identity`,
  `codegg-protocol`, `sqlx`, `serde`, `sha2`, `tokio`, `thiserror`
  used; no `authorization` import — see §10).
- `crates/codegg-core/src/authorization.rs`: `audit_query` /
  `audit_export` as `DirectProject` + `AuditRead`,
  `audit_capabilities` as global; representative requests for all
  three; `audit_provenance` bridge copying gate-enforced decision
  linkage into the store (M005 input).
- `crates/codegg-core/src/team.rs`: `PrincipalKind::parse_for_audit`
  lenient reader helper.
- `crates/codegg-core/src/transport_auth.rs`:
  `AuthMethod::parse_for_audit` / `TransportClass::parse_for_audit`
  lenient reader helpers.
- `crates/codegg-protocol/src/core.rs`: `AuditQueryRequestDto`,
  `AuditExportRequestDto`, `AuditEventDto`, `AuditCapabilitiesDto`;
  `CoreRequest::AuditQuery/AuditExport/AuditCapabilities`;
  `CoreResponse::AuditPage/AuditExport/AuditCapabilities`.
- `src/core/daemon.rs`: `DirectProject` scope resolution for the two
  project-scoped audit requests; dispatch arms for capabilities
  (negotiation), query (bounded page), export (bounded envelope +
  digest). Reads run after the M003 gate, which already enforced
  `audit.read`; unknown filters degrade to empty pages.
- `scripts/check_audit_invariants.py` (new, 6/6 green): additive
  migration, no `UPDATE`/`DELETE` on structural rows, bounded
  metadata + redaction, end-to-end authorized reads, sequence +
  idempotency, operator doc presence.
- `scripts/check_project_catalog_invariants.py`: expected
  `STORAGE_LAYOUT_VERSION` 53 → 54.
- Docs: `architecture/audit.md` (ownership, schema, taxonomy,
  attribution, redaction, digests, retention, query/export,
  failure/backpressure, security, operator runbook, verification);
  `architecture/authorization.md` matrix 135 → 138 rows with the
  three audit operations.
- `tests/identity_m004_audit_foundation.rs` (new, 13 boundary tests,
  no required-features gate): concurrency, duplicates, restart,
  migration, secret corpus, bounds, retention, pagination/filter,
  daemon authorized query + capabilities, daemon denials, writer
  pressure, export integrity, visibility default.

Distinguished as absent (downstream milestones, not M004 scope):
comprehensive instrumentation of the Phase-11 surfaces (M005),
external SIEM bridge, immutable WORM storage, distributed node
sequencing.

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg-core audit
cargo test --workspace audit --no-fail-fast
cargo test --test identity_m004_audit_foundation
cargo test -p codegg-core --lib authorization
cargo test -p codegg-core --lib
cargo test --test storage_migrations --test identity_m003_daemon_authorization
cargo test --lib
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
python3 scripts/check_audit_invariants.py --verbose
python3 scripts/check_authorization_matrix.py --verbose
python3 scripts/check_project_catalog_invariants.py --verbose
bash scripts/check-core-boundary.sh
python3 scripts/audit_tokio_tests.py
bash scripts/verify.sh quick
```

### Results

- `cargo test -p codegg-core audit`: pass, 14 passed (taxonomy,
  digests, secret scans, append/duplicate/secret/bounds/retention/
  pagination/provenance/export/writer).
- `cargo test --workspace audit --no-fail-fast`: pass, 0 failures
  across the sweep (matches are the audit-focused tests; most suites
  filter to 0 and report ok).
- `cargo test --test identity_m004_audit_foundation`: pass, 13
  passed (concurrency, duplicate, restart, migration, secret corpus,
  bounds, retention, pagination/filter, daemon authorized query +
  capabilities, daemon query/export denials, writer pressure, export
  digest, visibility).
- `cargo test -p codegg-core --lib authorization`: pass, 21 passed
  (no M003 regression; matrix now covers 138 operations).
- `cargo test -p codegg-core --lib`: pass, 592 passed, 0 failed.
- `cargo test --test storage_migrations --test
  identity_m003_daemon_authorization`: pass, 4 + 9 passed.
- `cargo test --lib`: pass, 4342 passed, 0 failed.
- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets --all-features --
  -D warnings`: pass, 0 warnings (one test-only unused-variable
  lint found during development and fixed before the final run).
- `python3 scripts/check_audit_invariants.py --verbose`: pass, 6/6.
- `python3 scripts/check_authorization_matrix.py --verbose`: pass,
  5/5 (matrix coverage now includes the three audit operations).
- `python3 scripts/check_project_catalog_invariants.py --verbose`:
  pass, 7/7 (including `STORAGE_LAYOUT_VERSION is 54`).
- `bash scripts/check-core-boundary.sh`: pass.
- `python3 scripts/audit_tokio_tests.py`: exit 0; advisory
  candidates only in pre-existing files, none in M004 files.
- `bash scripts/verify.sh quick`: pass (`==> Quick verification
  passed.`).

All evidence above is local execution and is labeled accordingly. No
hosted `CI / verify` run is attached; routine CI remains the operator
action per the closed development-verification roadmap.

## 5. Invariant review

- Audit structural records are append-only: production code issues
  `INSERT`/`SELECT` on `audit_event` only (guard-pinned; the one
  test-only `INSERT` of a future action exercises lenient reads, and
  no `UPDATE`/`DELETE` on structural rows exists anywhere).
- Caller cannot rewrite actor/decision: the builder takes the
  transport-bound principal plus gate-copied provenance; request DTOs
  carry no principal/role/capability/decision fields (M003 spoof
  coverage still green).
- Secrets never enter metadata: secret-bearing keys, values, and
  bodies are rejected with `audit_secret_detected` before any write
  (corpus-pinned); digests are SHA-256 only.
- Content bodies have separate retention: `expire_bodies` deletes
  only `audit_body` rows; structural rows and digests survive
  (tested).
- Duplicate event IDs are idempotent: `UNIQUE(event_id)` +
  `ON CONFLICT DO NOTHING` returns the stored event; no second
  sequence is assigned (tested, including under the writer).
- Audit query is authorized: the M003 gate enforces `audit.read` on
  the queried project before dispatch; denials carry the standard
  `authorization_denied` shape with no existence signal (tested for
  Viewer and outsider on both query and export).
- Execution never blocks forever on audit I/O: pool busy timeout
  plus writer semaphore/timeout bounds every path; saturation is a
  typed error with counters, never silent success (tested).

## 6. Failure and recovery review

- Concurrent appends receive distinct increasing sequences from the
  single `AUTOINCREMENT` authority (16-way concurrency test).
- Restart continues the sequence via `sqlite_sequence` (file-DB
  close/reopen test: 1,2 → 3 with an ordered cross-restart page).
- Storage errors surface as typed `AuditError` and are never
  reported as appended; `FailClosed` and `FailVisible` differ only in
  caller handling, never in outcome fabrication.
- Saturated writers fail fast (`Backpressure`) and record
  `dropped_backpressure`; timed-out acquisitions record `timeouts`.
- Query cancellation drops the future and releases the pool
  connection (no retained locks; bounded `LIMIT` on every read).
- Malformed input degrades safely: unparsable project ids yield
  `audit_invalid_project`; unknown action/principal filters yield
  empty pages/exports; unknown stored actions read back verbatim as
  `Unknown`.

## 7. Migration and compatibility review

- Additive migration v54 (`CREATE TABLE IF NOT EXISTS` +
  `IF NOT EXISTS` indexes) inside the existing transactional
  `migrate_and_record` harness; forward and restart safe;
  remigration tested.
- `STORAGE_LAYOUT_VERSION` 53 → 54. No existing table altered; no
  data backfill; no historical logs promoted to authoritative audit.
- Protocol additions are additive enum variants with
  `#[serde(default)]` on new optional fields; older clients ignore
  unknown variants per the existing forward-compatibility contract.
  `PROTOCOL_VERSION` unchanged.
- Operation matrix grows 135 → 138 rows through the exhaustive
  match (compile error until classified) plus the executable guard;
  M003 boundary suites pass unmodified.

## 8. Security review

- Fail-closed reads on every path: team principals need a current
  `audit.read` grant on the queried project; unscoped team reads
  fail closed; `LocalOwner` personal-local flows observe everything
  through the same gate.
- Denial messages contain no secret material and no existence signal
  beyond operation/capability names the caller supplied.
- Redaction runs before the write, not after: rejected material never
  touches SQLite, the WAL, or export envelopes.
- Privilege boundaries unchanged: only `Active` principals
  authorize; `LocalOwner` broad policy remains an explicit
  composition visible in every event's `policy` field.
- Denial-of-service bounds: metadata entry/key/value/total caps,
  64 KiB body cap, query clamp 100, export clamp 200, bounded writer
  inflight (default 32) with timeouts, indexed lookups, no new
  network or spawn surface.
- `codegg-core` boundary guard passes; the audit module imports only
  `team`, `transport_auth`, `error`, `identity`,
  `codegg-protocol`, and base crates.

## 9. Documentation and operations

Updated:

- `architecture/audit.md` — ownership, schema, taxonomy,
  attribution, redaction, digests, retention separation,
  query/export, failure/backpressure, security, operator runbook,
  verification commands.
- `architecture/authorization.md` — matrix 135 → 138 with the three
  audit operations and a pointer to the audit contract.
- `scripts/check_audit_invariants.py` — executable audit
  foundation guard (6/6).
- `scripts/check_project_catalog_invariants.py` — version
  expectation 53 → 54.
- `plans/implementation/identity-authorization-audit/004-append-only-audit-foundation.md`
  — marked closed, linking this record.
- `plans/implementation/identity-authorization-audit/005-audit-instrumentation-attribution-closure.md`
  — unblocked to ready for handoff.
- `plans/subsystems/identity-authorization-audit-roadmap.md` — M004
  closed, M005 ready.
- `plans/registry.md` — Identity row advanced to M005 ready; M005
  registered dependency-ready; M004/M005 blocker rows resolved; M004
  recorded under closure evidence.

Operator note: after upgrading, existing databases migrate to v54
automatically on next daemon start. Team operators should grant
`Maintainer` or `Owner` to members who need audit reads
(`Viewer`/`Contributor` cannot query or export). Content bodies
expire via `expire_bodies` without losing attributable structure;
verify exports with `export_digest` over the received order.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| — | None. | — | — |

There are no unresolved critical, high, medium, or low M004 findings.
The following are explicit non-claims with named consumers, not
defects:

- Breadth of instrumentation (which surfaces emit events) is M005
  scope; this milestone supplies the store, taxonomy, and the
  `audit_provenance` bridge M005 appends through.
- Agent-loop stamping of tool receipts and run-row origin joins
  remain M005 inputs (carried forward from the M003 record).
- External SIEM bridge, WORM storage, and distributed node
  sequencing are out of scope per the plan and long-term staging.
- One boundary-driven design adjustment is recorded, not concealed:
  the store performs no authorization-module calls because
  `codegg-core` forbids `crate::auth*` imports
  (`scripts/check-core-boundary.sh`). Enforcement lives where M003
  put it — the daemon gate, which authorizes `audit_query` /
  `audit_export` (`DirectProject` + `audit.read`) before dispatch —
  and the store documents its reads as coordinator-internal. The
  `audit_provenance` bridge preserves typed decision linkage for
  writers without breaking the boundary.

## 11. Roadmap disposition

Milestone closed and one downstream dependency may proceed. The
registry audit found exactly one registered plan whose sole hard
blocker was M004 closure:

- Identity M005 (audit instrumentation and attribution closure),
  blocked on M004 → `ready` in the same commit. Its interface
  dependencies (durable agent runs, jobs, worktrees, assets,
  provider connections with correlation identities) were already
  landed per its plan §2.

No corrective pass is required and no new dependency-ready plan was
created beyond the unblock. Project collaboration M001 remains
blocked on identity M005 + presence M003; presence and all other
tracks are unaffected by this closure.

## 12. Registry updates

Included in the closure commit alongside this record:

- M004 source plan marked closed, linking this record.
- Roadmap milestone table: M004 `ready` → `closed` with closure
  link; M005 `blocked` → `ready`.
- M005 implementation plan: `blocked` → `ready for handoff` (sole
  blocker M004 now closed; the store, taxonomy, redaction, and
  `audit_provenance` bridge it consumes are landed and tested).
- Registry active-subsystem row: Identity current milestone M004
  ready → M005 ready; blocker column cleared to the M005 handoff.
- Registry dependency-ready table: M004 row replaced by the M005
  row (instrumentation closure; M004 closure is the satisfied
  dependency).
- Registry execution-order item 2: chain advanced to M004 closed →
  M005 ready.
- Registry blocked-work table: identity M005 row removed; presence,
  collaboration, interactive-process, and tool-program rows retained
  unchanged.
- Registry closure-evidence table: Identity M004 row added pointing
  at this record.
