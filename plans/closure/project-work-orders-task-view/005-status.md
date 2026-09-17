# Project Work Orders and Task View M005 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/project-work-orders-task-view/005-external-task-trigger-endpoint.md`

Source subsystem roadmap:

- `plans/subsystems/project-work-orders-task-view-roadmap.md#7-milestones`

Repository baseline reviewed: `b2759bc3`
(M004 closure tree; the plan's pre-plan baseline `3ed78561` predates
M001–M004 and the implementation developed on top of the M004 closure)

Implementation commits or pull requests:

- `f22c9d8d` — work-orders M005: external task-trigger
  capability and endpoint (trigger domain/verifier/store, v63
  migration, management protocol/authorization, narrow HTTP fire
  route, idempotency/rate/repeat handling, guard, docs, focused
  tests) — commit hash recorded at commit time below.

## 1. Executive finding

M005 is complete. An authorized user creates a trigger and receives
its bearer once; an external script POSTs the bearer and latches only
the intended gate; the M002 coordinator wakes and materializes
exactly one canonical session/job. GET/query-string access cannot
fire; duplicate/concurrent fire cannot duplicate execution or
pre-arm a later repeat; triggers revoke/expire/exhaust, survive
restart, and remain secret-free in persistence, projections, logs,
audit, events, and errors. The trigger bearer cannot access any
other CodeGG API: firing is not a Core operation and the bearer
never enters principal resolution. This is capability/security work
as the plan classifies it; M006 is unblocked by this closure.

## 2. Requirement-to-evidence matrix

| Requirement (plan §) | Evidence | Result | Notes |
|---|---|---|---|
| Typed record/ID/verifier storage (§5, §13A) | `TaskTriggerId` typed identity; `TaskTrigger` + `TaskTriggerMetadata`; `task_trigger` table (verifier-only SHA-256 hex, `verifier_version = sha256-v1`); `Debug` omits verifier | pass | `trigger.rs` unit (8) + `trigger_secret_is_verifier_only_at_rest` |
| CSPRNG one-time secret, hash-only persistence (§5, §10) | 32 CSPRNG bytes, base64url-no-pad (`cggtr_<id>.<secret>`); plaintext returned once (`secret: Some` only on fresh create); converged retries return `secret: None` | pass | `generated_bearers_have_required_shape_and_entropy`, `trigger_create_secret_shown_once_with_keyed_convergence` |
| Reuse of token-hashing posture without sharing records (§5) | SHA-256 digest + constant-time compare, same posture as `PersonalTokenStore`; separate table/prefix/type; non-collision pinned both directions | pass | `presentation_routing_never_collides_with_personal_tokens` |
| Create/list-metadata/revoke under project auth (§7, §13B) | `WorkOrderTriggerCreate/List/Get/Revoke` Core ops; create/revoke `session.create`, list/get `session.read`; ID-only locators resolve via `task_trigger_project`; denials keep `project_not_found` | pass | `trigger_management_authorization_matrix` (contributor/viewer/outsider) + matrix guard |
| Listing returns metadata only (§7) | `TaskTriggerMetadataDto` (no secret/verifier fields); wire-JSON negative census | pass | `trigger_metadata_dto_carries_no_secret_material`, list/get JSON census |
| Narrow unaffiliated fire endpoint (§6, §13C) | `POST /api/v1/task-triggers/{id}/fire`, bearer-only, empty body, `accepted`/`already_fired` + opaque receipt | pass | 9 real-HTTP tests over loopback TCP |
| GET zero side effect; no query secrets (§3, §6, §10) | Route registers POST only (axum answers other methods `405`); handler never reads `Query`/cookies; query-smuggled secret ignored | pass | `http_get_has_no_side_effect`, `http_query_string_secret_is_ignored`, static guard |
| Expiry/max-fire/revocation (§5, §13D) | `expires_at_ms` (future-only), `max_fires` (`1..=1M`), monotonic revoke; derived `expired`/`exhausted` status, no sweeper | pass | `trigger_revoke_expire_exhaust_share_one_privacy_shape`, `trigger_max_fires_exact_with_exhausted_rejection` |
| Idempotency-Key retries (§8, §13D) | `task_trigger_receipt` ledger `(trigger_id, key)`; same key returns same receipt; receipts bounded (4096/trigger, oldest evicted) | pass | `trigger_fire_latches_once_with_stable_keyed_receipts`, restart convergence |
| Gate latch is unique/monotonic without key (§8) | Occurrence `gate_latches_json` CAS admits one winner; losers converge inert without consuming budget | pass | `trigger_concurrent_fires_latch_exactly_once` (16 tasks, 1 latch, count 1) |
| Repeat/re-arm semantics (§9) | Exactly the earliest waiting/ready occurrence is fireable; racing fire while latched/running is inert (no queue-one); re-armed repeat starts unlatched and fires once | pass | `trigger_fire_while_running_is_inert_with_repeat_rearm` |
| Audit/events without secrets (§10) | `work_order_lifecycle` rows with locator/count metadata; `WorkOrderTriggerChanged` (`created`/`revoked`/`fired`/`replay`); `Safe` publication | pass | `trigger_audit_rows_carry_no_secret`, event secret census |
| Rate limiting + bounded body (§10, §13D) | Per-router IP-keyed limiter (100/60s, bounded keys); 4 KiB body cap + empty-body rejection; `Retry-After` on 429 | pass | `http_rate_limit_is_bounded_and_secret_free` (keep-alive), `http_body_is_rejected_and_bounded` |
| Fire wakes coordinator; no direct execution (§3, §11) | `fire_work_order_trigger` latches durably, publishes event, calls `wake_work_orders_for_project`; route never constructs AgentLoop/scheduler/jobs | pass | `trigger_fire_reaches_coordinator_exactly_once` (1 session + 1 job, replay converges), coordinator guard |
| Revoked/expired/exhausted fail closed, minimal oracle (§3) | One generic `trigger_invalid` 401 for unknown/wrong/inactive; no locator/project/secret/verifier content | pass | byte-identical 401 bodies across all four failure classes |
| TUI/CLI display (§4) | No existing safe secret-display surface; protocol creation response is the M005-sufficient surface per plan | pass | Deliberate non-change, recorded here |
| Endpoint/security docs + curl (§13E) | `architecture/work_orders.md` M005 section, `architecture/server.md` fire contract + header curl, `authorization.md` matrix rows, `storage.md` v63 | pass | — |
| Log-redaction/static checks (§13E) | `scripts/check_task_trigger_boundaries.py` (9 checks) | pass | Green; see §4 |

## 3. Production implementation evidence

Ownership landed:

- `crates/codegg-core/src/identity.rs`: `TaskTriggerId` typed
  identity (+ round-trip fixture).
- `crates/codegg-core/src/work_order/trigger.rs` (new): bearer
  format (`cggtr_`), verifier version, bounds, `TaskTriggerState`
  (`active | revoked`) + derived `TaskTriggerStatus`
  (`active | revoked | expired | exhausted`), `TaskTrigger`
  (verifier-omitting `Debug`), metadata DTO projection,
  `NewTaskTrigger`, `FireOutcome` (`accepted`/`already_fired`),
  CSPRNG generation, SHA-256 verifier, constant-time verify,
  presentation routing, redaction, expiry/max-fire/key
  validation, audit metadata. 8 unit tests.
- `crates/codegg-core/src/work_order/store.rs`: `TASK_TRIGGER_SCHEMA_STATEMENTS`
  (`task_trigger` verifier-only table + `task_trigger_receipt`
  ledger, bounded retention `4096`), `task_trigger_project`
  resolver (wired into `work_order_project`), `create_task_trigger`
  (gate-binding validation, per-work-order cap 16, keyed
  convergence, mismatch conflicts), `get/list_task_triggers`
  (metadata projection by callers), `revoke_task_trigger`
  (monotonic, idempotent), `fire_task_trigger` (verify → receipt
  convergence → binding check → earliest waiting/ready occurrence
  → latch CAS with bounded retries → budget-guarded count →
  receipt; privacy-safe inactive shape; transient-busy retries).
  `MAX_TRIGGER_RECEIPTS_PER_TRIGGER` eviction is best-effort and
  can never duplicate execution (latch CAS is the exactly-once
  guard).
- `crates/codegg-core/src/session/schema.rs` + `storage/mod.rs`:
  `migrate_v63` applying the trigger statements;
  `STORAGE_LAYOUT_VERSION` 62 → 63 (layout guard green).
- `crates/codegg-protocol/src/work_order.rs`:
  `TaskTriggerMetadataDto` (verifier-free), `TaskTriggerCreateRequest`,
  `TaskTriggerListRequest`, `TaskTriggerFireResultDto` (narrow
  vocabulary). 2 new DTO tests (suite 183 → 185).
- `crates/codegg-protocol/src/core.rs`: additive
  `CoreRequest::WorkOrderTriggerCreate/List/Get/Revoke` and
  `CoreResponse::WorkOrderTrigger/WorkOrderTriggerList` (secret
  `Option`, `None` except fresh creation) plus
  `CoreEvent::WorkOrderTriggerChanged` (`Safe` publication).
  `PROTOCOL_VERSION` unchanged (same additive convention as
  M001–M004). No `WorkOrderTriggerFire` variant exists by design.
- `crates/codegg-core/src/authorization/policy.rs`:
  `work_order_trigger_create/revoke` (`DirectProject` +
  `session.create`), `work_order_trigger_list/get`
  (`DirectProject` + `session.read`), with representative
  requests (matrix guard green).
- `src/core/daemon_work_orders.rs`: trigger arms in
  `is_work_order_request`/`is_work_order_mutation` (create/revoke
  skip pre-side-effect audit, recorded post-mutation),
  `handle_work_order_request` create/list/get/revoke handlers,
  `resolve_trigger` (privacy-preserving), `after_trigger_mutation`
  (structural audit + event, duplicate-suppressed), and
  `fire_work_order_trigger` — the only production path accepting a
  trigger bearer (verify → latch → event → coordinator wake; wake
  failure is recoverable via the due/reconciliation scan and never
  fails the committed fire).
- `src/core/daemon.rs` + `daemon_family.rs`: trigger management
  ops route to the `WorkOrders` family with direct/ID-scoped
  project resolution; denials keep `project_not_found`.
- `src/server/routes/task_trigger.rs` (new): `POST
  /api/v1/task-triggers/{id}/fire` with its own IP-keyed limiter,
  4 KiB body cap, hardening headers, and no principal auth layer.
  Bearer/path agreement, empty-body rejection, key validation,
  principal-shaped bearer rejection, privacy-safe error mapping,
  locator-only success logging.
- `src/server/http.rs`: trigger router merged into the server app;
  production `run_server` now serves
  `into_make_service_with_connect_info::<SocketAddr>()`.
- `scripts/check_task_trigger_boundaries.py` (new): 9 static
  checks (prefix separation, verifier-only storage, POST-only +
  no-query, log hygiene, no principal resolution, non-Core fire
  shape, secret-free DTOs, v63 migration/layout, server wiring).
- Tests: `tests/work_orders_m005_trigger.rs` (13 service/daemon
  tests incl. 16-way concurrency + file-DB restart; 9 real-HTTP
  tests behind the `server` feature over loopback TCP).
- Docs: `architecture/work_orders.md` (M005 section),
  `architecture/server.md` (fire contract + header curl +
  ConnectInfo gotcha), `architecture/authorization.md` (M005
  paragraph + 4 matrix rows), `architecture/storage.md` (v63).

Deliberate adjustments from the plan text (semantics preserved):

- No storage migration was avoidable in M002, but M005 mints new
  tables, so `migrate_v63` + layout 63 is the plan's expected
  "migration if not included in M001" path. The M001 foundation
  test's hardcoded layout literal (`62`) moved to `63` with its
  comment; the dynamic layout guard needed no edit.
- Trigger state persists only `active | revoked`; `expired` and
  `exhausted` derive at read/fire time. This satisfies "revocable,
  bounded" with no background sweeper and makes restart
  preservation structural rather than flag-dependent.
- Terminal/cancelled work orders make fire inert-but-success-shaped
  (`already_fired`, fresh receipt) rather than an error, so a
  cancelled queue cannot be probed for lifecycle state through the
  public endpoint. Revoked/expired/exhausted/unknown/wrong-secret
  share the generic `401`.
- Receipt eviction (4096/trigger) is documented as a retry hint,
  not an exactly-once mechanism: post-eviction replays re-enter
  the latch CAS and converge inert, so eviction can never
  duplicate execution.
- `GET` answers `405` (axum's automatic method rejection) rather
  than a trigger-shaped denial; both are side-effect free and
  carry no oracle content.
- M005 repaired a latent server defect found by its HTTP
  qualification: `run_server` served a bare `Router`, which
  discards the accept-side address, so the IP-keyed rate
  limiter's `ConnectInfo` extraction failed and *every* HTTP
  request 500'd. The server now serves
  `into_make_service_with_connect_info::<SocketAddr>()`,
  matching the long-standing WebSocket harness pattern. The
  production axum `:{param}` route spellings moved to the
  installed axum 0.8 `{param}` form in the same pass (bare-Router
  serving panics on the legacy form at router-construction
  time). Both are covered by the new HTTP tests plus the
  boundary guard's wiring check.
- No TUI/CLI trigger surface: per plan §4, the protocol creation
  response is the sufficient M005 surface (the composer sheet's
  external-trigger row stays a disabled placeholder until a later
  milestone designs human trigger management).

## 4. Verification executed

### Commands run (local; no CI lane added per verification policy)

```bash
cargo test -p codegg-core --lib -- work_order
cargo test -p codegg --lib -- work_order
cargo test -p codegg-protocol
cargo test --test work_orders_m001_foundation --test work_orders_m002_materialization --test work_orders_m004_dashboard
cargo test --features server --test work_orders_m005_trigger
cargo test --features server --test identity_m003_daemon_authorization --test identity_m002_transport_auth
cargo test --test identity_m004_audit_foundation --test identity_m005_audit_instrumentation
cargo test --test storage_migrations
cargo test -p codegg --features server --lib -- server::
python3 scripts/check_authorization_matrix.py
python3 scripts/check_task_trigger_boundaries.py
python3 scripts/check_work_order_coordinator.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_git_forbidden_patterns.py
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_project_catalog_invariants.py
bash scripts/check-core-boundary.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
./scripts/verify.sh quick
```

Plan §15 literal-to-actual mapping (recorded, not hidden):

- `cargo test -p codegg-core -- task_trigger` / `cargo test -p codegg
  --lib -- task_trigger`: the literal `task_trigger` filter matches
  no lib test path (unit tests live under
  `work_order::trigger::tests`); the substitute is the `work_order`
  filter above (43 core incl. 8 new trigger unit tests; 30 root
  lib).
- `cargo test --test server -- task_trigger`: no `tests/server.rs`
  target exists; the substitute is the M005 suite with the server
  feature (22 tests, 9 over real loopback HTTP exercising routing,
  methods, headers, body caps, and rate limits).
- `cargo test --test authorization -- task_trigger`: no
  `tests/authorization.rs` target exists (same substitution as the
  M004 closure); the substitute is `identity_m003_daemon_authorization`
  (9) + `identity_m002_transport_auth` (7) + the authorization
  matrix guard.
- `cargo test --test audit -- task_trigger`: no `tests/audit.rs`
  target exists; the substitute is `identity_m004_audit_foundation`
  (13) + `identity_m005_audit_instrumentation` (13) + the
  `trigger_audit_rows_carry_no_secret` test (SQL audit census).
- `python3 scripts/check_secret_boundaries.py`: no such script
  exists in the repo; the equivalent coverage is
  `check_git_forbidden_patterns.py` (PASS, 0 findings) plus the new
  `check_task_trigger_boundaries.py` (ok).
- Clippy ran with the repo-conventional `--locked` flag added.

### Results (local)

- Core lib `work_order`: 43 passed (35 pre-existing + 8 new
  trigger unit). Root lib `work_order`: 30 passed.
- Protocol: 185 passed (183 + 2 new trigger DTO tests).
- M001 12 / M002 10 / M004 dashboard 8 — no regressions (M001's
  layout literal moved 62 → 63 with the migration; nothing else
  touched).
- M005 suite: 22 passed (13 service/daemon incl. 16-way
  concurrent-fire single-winner, max-fire exactness, repeat
  re-arm, file-DB restart, coordinator exactly-once with 1
  session + 1 job; 9 HTTP incl. accept/replay, GET-inert,
  privacy-shape byte-equality, query-ignore, body caps,
  principal-bearer isolation both directions, keep-alive rate
  limit with `Retry-After`, pool-less 503, log capture proving
  the bearer never reaches logs).
- identity_m003 9 / identity_m002 7 / audit m004 13 / audit m005
  13 / storage_migrations 4 / server lib 25. All green.
- Guards: authorization matrix verified; trigger boundaries ok;
  coordinator ownership ok; execution-ownership ok;
  git-forbidden PASS; daemon-cwd ok; catalog invariants 7/7;
  core-boundary pass; fmt clean; clippy
  (`--workspace --all-targets --locked`) clean; `verify.sh quick`
  pass.

## 5. Invariant review

| Plan §3 invariant | Evidence |
|---|---|
| Fire does exactly one semantic thing | `fire_task_trigger` latches the bound gate only; no prompt/model/lane mutation surface exists on the route (body rejected) |
| Trigger secret is not a principal credential | No `TriggerFire` Core variant; bearer never enters `AuthenticatedPrincipal` paths (guard); personal/global bearers rejected at the fire route; trigger bearer rejected by `resolve_bearer_principal` (test both directions) |
| POST-only; GET zero side effect | POST-only registration; GET → 405 with `fire_count` pinned 0 |
| Secret absent from query/projection/log/audit/error/storage | Query never read; DTOs verifier-free; tracing log capture test; SQL audit census; event census; byte-identical generic 401s; store sweep finds verifier only |
| Replays cannot duplicate execution | Latch CAS single-winner under 16-way concurrency + transient-busy retries; keyed receipts stable across restart; coordinator wake converges (0 second advance, same session/job ids) |
| All/Any + policy revalidation intact | Fire latches one gate only; M002 evaluation/join/claim/model-policy narrowing untouched (M002 suite green); cancelled work orders inert |
| No direct agent/session execution from HTTP | Route calls store latch + coordinator wake only; coordinator/execution-ownership guards green |
| Revoked/expired/exhausted fail closed, minimal oracle | Single 401 shape; revocation monotonic; expiry/exhaustion derived; fire-vs-revoke/expiry races resolve in-transaction |

## 6. Failure and recovery review

- Duplicate delivery: latch CAS + receipt convergence; keyed
  replays return the stored receipt (latched flag included);
  keyless duplicates are inert for the latched occurrence.
- Stale writers: creation-key mismatch conflicts; CAS revision on
  revoke path (`revision + 1`); lane/work-order CAS unchanged.
- Concurrent fires: exactly-one-winner latch + budget-guarded
  count; same-key racers converge on the first receipt.
- Restart: file-DB close/reopen/remigrate preserves
  trigger/revoke/expiry/count/receipt/latch state; wake failure
  after commit recovers via the coordinator due scan (wake errors
  intentionally unpropagated to the caller).
- Partial persistence: latch commits before count; a lost
  count-race surfaces the generic inactive shape while the latch
  stands and still wakes (fail-safe toward release, fail-closed
  toward budget).
- Malformed input: overlong/malformed locators → `400`
  framing errors with no lookup; malformed keys → `400`;
  non-empty bodies → `400`; oversized bodies → `400`/`413`.
- Cancellation: terminal work orders make fire inert without
  reactivation or state disclosure.
- Sequence: failure-hold semantics unchanged; fire never
  pre-latches a future repeat (re-arm creates a fresh unlatched
  occurrence).
- Contention: receipt eviction bounded; rate-limiter key maps
  bounded (10k); trigger-per-work-order cap (16); idempotency
  keys bounded (128).

## 7. Migration and compatibility review

- Additive migration v63 (`task_trigger`, `task_trigger_receipt`,
  indexes); existing databases gain empty trigger tables;
  downgrade-then-remigrate converges (M001 test pins 63).
  `STORAGE_LAYOUT_VERSION` 62 → 63.
- Protocol additive: 4 request + 2 response variants + 1 event;
  `PROTOCOL_VERSION` unchanged; old clients never send them; old
  servers fail them closed through the standard unknown-op path.
- `Cargo.toml`/`Cargo.lock`: `tower-http` gains the `limit`
  feature (adds `http-body-util` to the lock; no version moves).
- Rollback: downgrading drops the ops; trigger rows sit inert;
  no data migration to reverse.

## 8. Security review

- Bearer entropy: 32 CSPRNG bytes (43 base64url chars);
  uniqueness sampled (32/32 distinct); verifier SHA-256 hex.
- Authorization: management is `session.create`/`session.read`
  project ops with server-side locator resolution and
  `project_not_found` denials (contributor creates/revokes,
  viewer reads, outsider sees nothing — integration-pinned).
- Privacy: failure byte-equality across unknown/wrong/
  revoked/expired; inert-cancelled shape discloses no lifecycle;
  denials leak no locators.
- Isolation: trigger bearer cannot call Core APIs (no variant
  exists); principal credentials cannot fire (rejected without
  verification); `TaskTriggerId`/`cggtr_` distinct from every
  principal identity by type and prefix.
- Bounds as DoS control: path/key/body/body-cap/trigger-count/
  receipt-count/rate-limit/key-map caps; cursor-free bounded
  listing (100).
- Secrets: one-time display; verifier-only rest; redaction
  helper; `Debug` omits verifier; no secret in SQL, DTOs,
  events, audit, logs, or errors (each with a dedicated test or
  guard check).

## 9. Documentation and operations

- `architecture/work_orders.md`: M005 lifecycle/contract section
  + storage v63 + related-docs links.
- `architecture/server.md`: fire contract, header-based curl
  example, `ConnectInfo` gotcha, related docs.
- `architecture/authorization.md`: M005 paragraph + 4 matrix
  rows (17 → 21 work-order rows).
- `architecture/storage.md`: v63 entry.
- Operator diagnostics: fire outcomes log locator + narrow
  status only; trigger errors surface as `code: message`
  (`trigger_invalid`, `trigger_invalid_request`,
  `trigger_unavailable`); `Retry-After` on 429.
- Static guards: authorization matrix, trigger boundaries,
  coordinator ownership, execution-ownership — all green.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | No dedicated `tests/server.rs` / `tests/authorization.rs` / `tests/audit.rs` / `scripts/check_secret_boundaries.py` for the plan's literal verification lines | Substituted with named existing targets + guards (same surfaces) | None; future plans should name existing targets |
| low | Per-(IP, port) rate limiting: fresh connections carry fresh ephemeral ports and therefore fresh budgets; sustained abuse must pipeline on one connection to trip the limiter | Same posture as the pre-existing global limiter (shared implementation) | None for M005; a future hardening pass may key on IP alone |
| low | Claim-time approval/sandbox ceilings remain `None` (carried from M002) | Per-turn ceilings still enforced on `TurnSubmit`; snapshots never widen | Future milestone may thread ceilings into claim; unchanged behavior |

No critical/high/medium findings. No corrective pass required.

## 11. Roadmap disposition

Milestone closed and the next dependency may proceed: M005 exit
condition is satisfied (an external script safely releases exactly
the intended waiting gate without general CodeGG credentials or
duplicate execution).

Blocked-work audit (registry `Blocked work` + roadmap §6 dependency
graph):

- M005 (external trigger endpoint, hard dep M002 already closed;
  ordering gate M004 closed) → **closed** by this record.
- M006 (agent WorkOrder tool, hard dep M002/M001 service satisfied;
  scheduled after M005) → **ready**. Its ordering gate (M005
  closure) is now satisfied; no semantic dependency on trigger
  secrets exists (M006 explicitly must not receive trigger
  secrets through the model tool).
- M007 (qualification, hard dep M001-M006) → remains **blocked**
  on M006 closure (M001-M005 closed).

## 12. Registry updates

- Move M005 (`005-external-task-trigger-endpoint.md`) from ready
  to closed with this closure record (implementation commit hash
  below).
- Move M006 (`006-agent-work-order-tool-and-batches.md`) from
  blocked to ready: hard dependencies (M002 closure, M001
  service) satisfied and ordering gate (M005 closure) now
  satisfied.
- Keep M007 blocked on M001-M006 closure (M006 still open).
- Record M005 under recently closed work; update the subsystem
  roadmap M005 status to closed and M006 to ready; mark the M005
  plan file implemented.
