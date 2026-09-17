# Project Work Orders and Task View M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/project-work-orders-task-view/001-work-order-domain-storage-protocol.md`

Source subsystem roadmap:

- `plans/subsystems/project-work-orders-task-view-roadmap.md#7-milestones`

Repository baseline reviewed: `22ac78cd`

Implementation commits or pull requests:

- `a856f2e0` — work-orders M001: WorkOrder domain, storage,
  authorization, and protocol foundation (domain/store/protocol/
  daemon/terminology/docs/tests, no execution)

## 1. Executive finding

M001 is complete. Authorized clients can create/list/get/update/
reorder/cancel durable waiting work orders and occurrence records with
stable typed identities and CAS revisions, but no work order starts
execution yet: the store cannot reach the scheduler, session store,
worktree service, or any provider, and no release evaluation,
materialization, trigger endpoint, TUI surface, or model-visible tool
was added. This is infrastructure/invariant work (not a user-visible
capability), exactly as the plan classifies it.

## 2. Requirement-to-evidence matrix

| Requirement (plan §) | Evidence | Result | Notes |
|---|---|---|---|
| Typed IDs distinct (§3) | `WorkOrderId`, `WorkOrderOccurrenceId`, `SequenceLaneId` via `typed_identity!`; `identity_round_trip_tests` extended | pass | `cargo test -p codegg-core --lib identity` green |
| Domain bounds (§5, §6) | `work_order/model.rs` bounds + `validate_*`; gate/repeat/lifecycle unit tests | pass | 19 lib tests: `cargo test -p codegg-core --lib work_order` |
| Gate description semantics (§6) | `validate_gate_set`: immediate-sole, no duplicate kinds, bounded delay/timestamp, opaque same-project trigger refs, `all`/`any` join | pass | `gate_validation_rejects_empty_and_incompatible_combinations` |
| Finite repeat (§6) | `validate_repeat_count` `1..=256`; explicit 0-based indexing test | pass | `repeat_policy_is_finite_and_explicit`, `occurrence_indexing_is_explicit_zero_based` |
| Lifecycle matrices (§7) | `can_transition_work_order` / `can_transition_occurrence`; one-way terminals; explicit `NeedsAttention -> Waiting` resume | pass | `work_order_state_matrix_*`, `occurrence_matrix_*` |
| Additive schema + layout bump (§8) | `migrate_v60` (5 tables/indexes), `migrate_v61` (attribution scope rebuild), `STORAGE_LAYOUT_VERSION = 61` | pass | migration tests + catalog guard green |
| No schedule backfill (§8) | No backfill code; fresh DBs gain empty tables; schedule/session/job behavior untouched | pass | `daemon_remigration_*`, neighboring suites green |
| Store operations (§9) | `WorkOrderService`: idempotent create, atomic batch, get/list, CAS update, cancel/pause/resume, occurrence CRUD, lane CRUD/reorder/attach/move, project resolution, summary | pass | 19 lib tests incl. `batch_creation_is_atomic_and_ordered` |
| Batch atomicity (§9, §13) | One transaction for rows + lane positions + batch ledger; retry converges; mismatch conflicts | pass | store + daemon batch tests; row count asserted |
| CAS update/reorder (§9, §13) | `UPDATE ... WHERE revision` + affected-row check; exact-set reorder; rollback-before-reread | pass | stale-writer tests; concurrent race test |
| Protocol + capabilities (§10) | 17 `CoreRequest`, 9 `CoreResponse`, 2 `CoreEvent` variants; `work_order_capabilities`; DTO round-trip tests | pass | 6 protocol tests; daemon capability test |
| Authz matrix integration (§10) | 17 `operation_descriptor` arms + representatives; server-side project resolution; not-found denials | pass | `check_authorization_matrix.py` 5/5; privacy tests |
| Origin attribution + audit (§10) | `OriginAttributionStore` scope `work_order` (first-wins); structural `work_order_lifecycle` events with decision id + revision | pass | attribution immutability + audit-page tests |
| Bounded projections (§10) | List cursors/limits, summary counts, secret-free DTO/event JSON assertions | pass | `daemon_projections_carry_no_secrets_or_reasoning` |
| Canonical amendment (§11) | `001` definitions + tree + prohibited-use line; `000` §6/§13/§17 paragraphs; non-goal retained | pass | diff reviewed; no unrelated canonical rewrite |
| Architecture docs (§12 E) | New `architecture/work_orders.md`; authorization/audit/storage matrix updates | pass | — |
| No execution (§4 out) | No coordinator, no session/job/worktree creation, no trigger/tool/TUI surface | pass | session/job row counts asserted zero; grep-verified |

## 3. Production implementation evidence

Ownership landed:

- `crates/codegg-core/src/work_order/` (`mod.rs`, `model.rs`,
  `store.rs`): domain types, validation, and the durable
  `WorkOrderService`. UI/server/plugin/auth-free (boundary guard
  green). Lane membership normalized in `sequence_lane_member`; the
  lane revision is the single ordering authority.
- `crates/codegg-core/src/session/schema.rs` `migrate_v60`/`migrate_v61`
  plus `STORAGE_LAYOUT_VERSION = 61`.
- `crates/codegg-protocol/src/work_order.rs` DTOs plus
  `CoreRequest`/`CoreResponse`/`CoreEvent` variants in `core.rs`;
  `Safe` publication class for both new events
  (identity/change/revision only).
- `crates/codegg-core/src/authorization/policy.rs`: 17 arms plus
  representatives; `attribution.rs` admits the `work_order` scope;
  `audit.rs`/`audit_instrumentation.rs` add the live-mapped
  `work_order_lifecycle` action with a structural builder; the lane
  emit path reuses the action with lane-scoped keys.
- `src/core/daemon_work_orders.rs`: `WorkOrders` family with a boxed
  pre-router (`is_work_order_request`), server-side project
  resolution, privacy-shaped not-found, immutable creator attribution,
  post-mutation audit with durable revisions, and structural liveness
  events. Family table, denial privacy, and audit seam updated in
  `daemon.rs`; service constructed in `daemon_construct.rs`.
- Planned but absent (correct per scope): release evaluation,
  occurrence claiming, session/job materialization, worktree
  allocation, trigger endpoint/storage, model-visible tool, TUI task
  surfaces, schedule migration. M002 owns the first execution
  crossing, through `JobSubmissionService` only.

Deliberate adjustments from the plan text (semantics preserved):

- Two migrations instead of one: v60 (domain tables) plus v61 (admits
  the `work_order` attribution scope in the v53 table's SQL `CHECK`
  via a row-preserving rebuild). The Rust allow-list alone was
  insufficient — the durable `CHECK` failed the insert closed, which
  the attribution integration test caught.
- Added `WorkOrderLaneAttach` (CAS append/insert for existing waiting
  orders) so lane authority is complete beyond create-time placement;
  counted in the 17 operations and classified identically.
- `write_lane_order` rolls back before re-reading the current lane
  revision on CAS conflict: the re-read needs its own pool connection
  and holding the write transaction across that acquire self-deadlocks
  small pools. Found via a 30 s stall in the concurrent-reorder race;
  covered by a permanent exactly-one-winner regression test.

## 4. Verification executed

### Commands run (local; no CI lane added per verification policy)

```bash
cargo test -p codegg-core --lib work_order
cargo test -p codegg-protocol work_order
cargo test -p codegg-core -- migration
cargo test -p codegg-core --test continuation_checkpoint --test work_plan_foundation
cargo test --test work_orders_m001_foundation
cargo test --test identity_m003_daemon_authorization --test scheduler_authority_matrix \
  --test storage_migrations --test collaboration_m001_chat --test collaboration_m003_chat_actions
cargo test --test identity_m004_audit_foundation --test identity_m005_audit_instrumentation
cargo test -p codegg-protocol
cargo test -p codegg-core --lib
python3 scripts/check_authorization_matrix.py
python3 scripts/check_project_catalog_invariants.py
bash scripts/check-core-boundary.sh
python3 scripts/check_audit_invariants.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_git_forbidden_patterns.py
python3 scripts/check_websocket_bounds.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
scripts/verify.sh quick
```

(`cargo test --test authorization -- work_order` from the plan has no
equivalent target name; the substitute is the new
`work_orders_m001_foundation` binary plus `identity_m003`, recorded
here explicitly.)

### Results (local)

- `codegg-core --lib work_order`: 19 passed.
- `codegg-protocol work_order`: 6 passed.
- `codegg-core -- migration`: all targets pass (incl. bumped
  `continuation_checkpoint`/`work_plan_foundation` layout pins, now 61).
- `work_orders_m001_foundation`: 12 passed in ~1 s (CRUD, batch+lane,
  concurrent reorder race, idempotency, file-backed reopen,
  remigration + v61 preservation, outsider privacy, cross-project
  closure, attribution immutability + audit page, secret-free
  projections, occurrences/summary/moves).
- Neighboring suites: `identity_m003` 12, `scheduler_authority_matrix`
  10, `storage_migrations` 9, `collaboration_m001` 13,
  `collaboration_m003` 4, `identity_m004` 13, `identity_m005` 13,
  `codegg-protocol` 183, `codegg-core --lib` 760 — all pass.
- Guards: authorization matrix 5/5, catalog invariants 7/7,
  core-boundary pass, execution-ownership ok, daemon-cwd pass,
  git-forbidden pass, websocket-bounds ok, fmt clean, clippy
  (`--workspace --all-targets -- -D warnings`) clean, `verify.sh
  quick` pass.
- Pre-existing failures unrelated to this change (verified identical
  on clean `22ac78cd` via `git stash`): `check_audit_coverage.py`
  (`_authz_operations` reads an empty op set from
  `authorization.rs`), `check_audit_invariants.py` (audit-reads
  authorization lookup), `check_scheduler_bypass.py`
  (`src/agent/snapshot_capture.rs` spawner annotation). No new
  coverage lane was added; these remain for their owners.

## 5. Invariant review

| Plan §3 invariant | Evidence |
|---|---|
| Distinct typed IDs | `WorkOrderId`/`WorkOrderOccurrenceId`/`SequenceLaneId` via `typed_identity!`; round-trip tests |
| Intent, not execution authority | Store has no session/job/worktree/provider path; session/job counts asserted zero after CRUD |
| No speculative sessions | Waiting rows exist with zero session rows (CRUD + reopen tests) |
| No job/worktree/model execution | No second scheduler/admission/loop/retry owner (`check_scheduler_bypass` unchanged); grep-clean |
| No second schedule/job/retry owner | Reuse only: `JobSubmissionService` untouched, reserved for M002 |
| Project-authorizable APIs; ID resolution daemon-side | 17 matrix arms; `resolve_authorization_project` + `work_order_project`; privacy tests |
| Explicit CAS conflicts | `RevisionConflict` on stale work-order/lane writes; zero-mutation assertions; race test |
| Bounded fields | Domain validation + SQL CHECK backstops + bounds tests |
| No trigger secrets in M001 | No secret column/DTO/param; secret-scan-style JSON assertions |
| No hidden reasoning field | No such field in domain, schema, DTOs, or events |

## 6. Failure and recovery review

- Duplicate delivery: submission/batch keys converge (`duplicate:
  true`, no new commit, no new event/audit); reused keys with
  different payloads conflict explicitly. Covered store + daemon.
- Stale writers: work-order update and lane reorder/attach/moves make
  zero mutation on stale revisions (`RevisionConflict`).
- Concurrent reorder: two writers racing one revision serialize —
  exactly one wins, the loser conflicts, the surviving order is a
  complete permutation. Permanent regression test (plus the
  rollback-before-reread fix above).
- Restart: file-backed reopen preserves identities/revisions;
  remigration over a downgraded marker re-applies v60/v61 additively;
  legacy attribution rows survive the v61 rebuild verbatim.
- Partial persistence: batch/lane writes are single-transaction
  (no partial positions); transaction failure leaves prior state.
- Malformed/unauthorized input: unknown enums fail closed; oversized
  fields rejected; cross-project references fail closed; outsiders see
  only `project_not_found`.
- Cancellation: pre-execution cancel marks future eligibility inert;
  terminal transitions one-way; in-flight propagation is M002 scope.

## 7. Migration and compatibility review

- v60 is additive `IF NOT EXISTS`; pre-M001 databases gain empty
  tables; no schedule backfill; existing session/job/schedule behavior
  unchanged (neighboring suites green).
- v61 rebuilds `origin_attribution` with the extended kind set,
  copying every row verbatim (same columns, same primary key); fresh
  databases receive the new shape directly; indexes recreated.
- `STORAGE_LAYOUT_VERSION` 59 → 61 atomically with wiring (chain,
  dispatch arms, definitions agree; catalog guard green); the two
  pre-existing layout pins bumped to 61.
- Protocol is additive: 17 request, 9 response, 2 event variants plus
  optional DTO fields; `PROTOCOL_VERSION` stays 2; old clients ignore
  unknown variants. Legacy minimal-create fixtures decode.
- Rollback limitation: downgrading the binary below v60/v61 leaves the
  new tables/rows inert but present; the version marker prevents older
  migrators from running forward. No data migration to reverse.

## 8. Security review

- Authorization: 17 operations classified (`session.create` for
  create/mutate, `session.read` for reads); project resolution
  server-side for all ID-only operations; no opaque local-owner-only
  primary surface; denials carry no project signal
  (`project_not_found`, matrix guard green).
- Privacy negatives tested: outsider list/get/cancel, absent-id
  oracle, cross-project update/reorder — all identically shaped.
- Secrets: no trigger secret material introduced; DTO/event JSON
  asserted free of secret/reasoning/credential substrings; prompt
  bodies are author-supplied intent (stored) but never enter audit
  metadata (digests/locators only).
- Attribution/audit: creator origin immutable (first-write-wins,
  forge attempt verified inert); mutations emit structural
  `work_order_lifecycle` events with decision ids and durable
  revisions; denial path emits without leaking scope.
- Bounds as DoS control: prompt 32 KiB, batch 32, list 100, repeat
  256, lanes 64/project, members 1024/lane, gate spec 4 KiB.

## 9. Documentation and operations

- New `architecture/work_orders.md` (ownership, identity, records,
  gates, lifecycle, storage, operations, protocol/authorization,
  failure semantics, non-execution boundary, tests).
- `architecture/authorization.md`: work-order section + 17 matrix
  rows; native-operation rendering count corrected to 188.
- `architecture/audit.md`: 26th action + live-emit paragraph.
- `architecture/storage.md`: v58/v59/v60/v61 entries.
- Canonical (ADR-0005-authorized): `001` work-order/occurrence/lane
  definitions, relationship tree, prohibited-use line; `000` §6/§13/§17
  paragraphs; non-goal against a general distributed workflow engine
  retained.
- Operator diagnostics: stable `work_order_*` wire codes
  (`not_found`, `revision_conflict`, `state_conflict`,
  `idempotency_conflict`, `project_mismatch`, `invalid_input`,
  `prompt_too_large`, `capacity`, `unavailable`, `storage_error`);
  capability negotiation advertises all bounds.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | `check_audit_coverage.py`, `check_audit_invariants.py`, `check_scheduler_bypass.py` fail identically on clean HEAD | None on M001; guards were already red | Owners of those guards to repair; not introduced here |
| low | M001-created occurrences have no daemon producer yet (explicit primitive only) | None; store API ready for M002 | M002 coordinator to own claim transitions |
| low | Lane member removal has no protocol operation (reorder is exact-set; attach adds) | Cancelled members occupy positions until M002 sequence evaluation skips them | M002/M003 to decide detach semantics if needed |

No critical/high/medium findings. No corrective pass required.

## 11. Roadmap disposition

Milestone closed and the next dependency may proceed: M001 exit
condition is satisfied (authorized clients create/list/get/update/
reorder/cancel durable waiting work orders and occurrence records with
stable identities/revisions; nothing executes yet). M002 (release
coordinator and materialization) is unblocked by this closure; M003+
remain blocked on their ordered predecessors per the roadmap
dependency graph.

## 12. Registry updates

- Move M001 from dependency-ready to closed with this closure record
  and implementation `a856f2e0`.
- Move M002 (`002-release-coordinator-session-materialization.md`)
  from blocked to ready: its sole hard dependency (M001 closure) is
  now satisfied; interface foundations (scheduler, session, worktree,
  authorization) were already closed.
- Leave M003–M007 blocked on their ordered predecessors.
- Record M001 under recently closed work.
- Update the subsystem roadmap M001 status to closed.
