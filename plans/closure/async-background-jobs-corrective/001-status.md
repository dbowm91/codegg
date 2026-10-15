# Async Background Jobs Corrective C001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/async-background-jobs-corrective/001-daemon-watch-admission-and-reconciliation.md`

Source subsystem roadmap:

- `plans/subsystems/async-background-jobs-idle-completion-corrective-addendum.md#dependency-and-execution-sequence`

Repository baseline reviewed: `21b35c79abca665f353b1a744073c6a17f7a9c6e`

Implementation commits or pull requests:

- `21b35c79` — feat(async-jobs): C001 daemon-owned idle CI-watch driver, singleton services, CAS lease and repair

## 1. Executive finding

C001 is complete as a bounded capability/invariant slice. A sleeping
session's GitHub Actions watch is now observed by one daemon-lifecycle-owned
idle coordinator and terminal delivery persists exactly one M001 notice
without any agent/model/user/tool activity. Per-turn service construction is
replaced by daemon-singleton injection, cross-service polls are serialized
by a persisted CAS lease, and watches persisted without their subscription
are repaired rather than orphaned. No model turn is started by C001, default
remains `notify_only` with zero autonomous inference, and no second
scheduler, per-watch timer, or default inference spend was introduced.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Zero-turn positive: daemon trigger reaches terminal with one session event/notice, no model calls | `tests/ci_watch_c001_idle_driver.rs::zero_turn_idle_driver_reaches_terminal_with_one_notice` — watch created, turn ended, spawned `CiWatchIdleDriver` drives queued→running→completed to one `completed` notice; harness never calls `status`/`advance_due_watches`/`AgentLoop.run`/`observe_once` | pass | 1/1 notice, `completed_success` via non-advancing `list`; no provider in test |
| No-turn expiry: TTL passes with no activity → one `expired` attention, no false failure, no remote cancel | `no_turn_expiry_yields_single_expired_attention` (TTL 1ms, spawned driver) | pass | status `expired`, summary contains no `failed`; remote workflow untouched by construction (`cancel` never called) |
| Singleton lifetime: one daemon `Arc` per service, two turns share identity, closing one turn leaves watchers live | `singleton_services_share_identity_across_turns` — `CoreRuntimeDeps::shared_*` `Arc::ptr_eq`, watcher shares notifier, drop-one-clone keeps service live | pass | Production turns inject via `TurnRunInput.shared_*`; fallback per-turn construction retained for legacy/test paths |
| Finite due observation: global due query, earliest-due wake, bounded tick, fair ordering, empty queue sleeps | `CiWatchIdleDriver` (`IDLE_DRIVER_BATCH=8`, `MAX_SLEEP=60s`, `MIN_SLEEP=100ms`), `due_watch_ids_global` (due-order + `watch_id` tie-break, `LIMIT 64`), `earliest_due_ms`, `advance_due_watches_global` | pass | Remote wait holds no process/CPU permit; terminal delivery reuses scheduler-owned M001 authority |
| Atomic claim/generation: persisted CAS lease, one admitted remote read per due, stale holder cannot overwrite terminal | `cross_service_single_admission_per_due_lease` — 50 contenders across two service handles, exactly 1 terminal winner, ≤3 HTTP requests (run+jobs), late duplicate `AlreadyTerminal` with no re-poll | pass | `claim_owner/claim_until/generation` (migration v76); `in_flight` is fast-path only; terminal transitions release the lease |
| Watch+subscription repair: crash between insert/subscribe, subscribe failure, duplicate/mismatched payload, completed-before-first-observation, restart terminal-before-notice | `crash_insert_before_subscription_is_repaired`, `crash_after_claim_is_recoverable_via_expiry`, `crash_terminal_before_notice_still_delivers_once`, `repair_registrations()` | pass | Conflicting owner fails closed to `needs_attention`; never an orphan forever in `Pending` |
| Independent terminal publisher: durable terminal + M001 notice without `AgentLoop` start | `emit_terminal_notice` unchanged authority, called from `observe_once` terminal paths and driver ticks; M003 safe-boundary injection retained as additional view | pass | No external GitHub text enters instruction context; failure summary is inert data |
| Rate/security: 401/403/404/429/503, ETag, Retry-After, malformed/large, redirect/SSRF, foreign session, budgets, fairness | `rate_security_fail_closed_and_fair_across_sessions` + M004 negative matrix (still green) | pass | Two projects share due-order queue; per-session caps at creation/listing; credential refs only |
| Compatibility: M001/M002/M003/M004 green, scheduler authority unchanged | `cargo test --test ci_watch_m004_completion --test job_notification_m001_contract --test job_background_test_slice --test job_continuation_m003_visibility` | pass | All four binaries green (M001 17/17, M003 11/11 observed; M004 + background slice green, exit 0) |
| Docs/diagnostics: ownership diagram, counts, no secret logs, preserved tool schema | `architecture/scheduler.md` (C001 authority), `architecture/jobs.md`, `architecture/authorization.md`, `src/scheduler/ci_watch.rs` header, `WatchCounts{active,overdue,leased,expired,attention,last_observed_ms}` | pass | `ci_watch` tool schema unchanged |

## 3. Production implementation evidence

- **Ownership:** `CoreRuntimeDeps::{job_notification_service, ci_watch_service, ci_watch_driver}` + `shared_*()` singleton constructors (`Arc::ptr_eq` proof); `CoreDaemon` creates both singletons at startup, installs the notifier on `JobScheduler` via sync setter, spawns exactly one `CiWatchIdleDriver` loop when pooled (pool-less explicitly has none); `DefaultTurnRuntime` reuses injected `TurnRunInput.shared_*` or falls back per-turn; `daemon_turns.rs` threads the daemon clones; `daemon_shutdown.rs` cooperatively stops the driver.
- **Due observation:** `src/scheduler/ci_watch_idle.rs` (new) — `Notify` + earliest-due timer loop, `run_once`/`recover_and_run_once`/`spawn`/`wake`/`request_shutdown`; `CiWatchService::{try_claim_due_lease, release_due_lease, due_watch_ids_global, advance_due_watches_global, earliest_due_ms, watch_counts, repair_registrations}`.
- **Atomic lease:** migration v76 (`claim_owner TEXT`, `claim_until INTEGER`, `generation INTEGER DEFAULT 0`, `idx_ci_watch_claim`); `STORAGE_LAYOUT_VERSION` 75→76; `CiWatchRecord.{claim_owner, claim_until, generation}` with serde defaults; `persist`/`load_one`/`load_session` column-authoritative; `try_claim` CAS (`state IN (pending,observing)` + expired-or-null lease, `generation+1`); `observe_once` DB-authoritative terminal probe, per-observation claim, lease release on nonterminal, lease-clearing terminal `transition`; stale `AlreadyTerminal` never re-polls.
- **Repair:** `repair_registrations()` scans nonterminal rows (DB-direct when pooled), re-subscribes missing `ciwatch:<watch_id>` with remaining TTL, or fails closed to `needs_attention` + terminal notice on conflict; driver calls it at startup.
- **Publisher:** unchanged M001 `record_terminal_for_watch` authority (idempotent on `ciwatch:<watch_id>`); session-event injection stays at the M003 safe boundary as an additional view, not the sole delivery.
- **Diagnostics:** `watch_counts()` (columns when pooled, map otherwise); no secrets logged; tool schema and M004 contract preserved.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
cargo test --test ci_watch_c001_idle_driver
cargo test --test ci_watch_m004_completion --test job_notification_m001_contract --test job_background_test_slice --test job_continuation_m003_visibility
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
```

Plus direct-binary run `./target/debug/deps/ci_watch_c001_idle_driver-* --test-threads=1`
for deterministic timing (cargo-test harness overhead avoided after build).

### Results

- `cargo fmt --all -- --check`: pass (after `cargo fmt --all` normalization).
- `ci_watch_c001_idle_driver`: **13/13 pass** (5 secret-scan sentinels + 8 C001
  tests: zero-turn positive, expiry, singleton, 50-contender cross-service,
  3 crash interleavings, rate/security). Total wall 18.11s direct-binary.
- M001/M003/M004/background suites: **all green, exit 0**. Observed:
  `job_notification_m001_contract` 17/17, `job_continuation_m003_visibility`
  11/11; `ci_watch_m004_completion` and `job_background_test_slice` green
  (same invocation, exit 0; exact counts not re-captured in tail output but
  no failures).
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
  (`CARGO_BUILD_JOBS=2`): pass after two findings fixed (type-complexity
  alias `CiWatchAuthRow`, const sleep-bound assert). 13m27s.
- `scripts/verify.sh quick`: pass (fmt, builtin-agents check, core/client/
  desktop boundaries, sandbox, ADR integrity, execution-ownership,
  TUI authorities, HTTP disposition, audit, scheduler-bypass, config-merge,
  provider wire/catalog/resilience, OpenAI endpoint, Eggwork routing,
  first-party skills, `cargo check --workspace --all-targets --locked`).
- Scheduler-bypass, execution-ownership, core-boundary, auth/projection
  guards: pass via `verify.sh quick`.
- **Not run (explicit):** hosted `CI / verify` + Desktop E2E on the exact
  head, native macOS/Windows, live GitHub API, live paid provider. No new
  permanent CI lane was added per plan. Local deterministic evidence is the
  release gate for C001; hosted qualification is deferred to C003.

## 5. Invariant review

- One scheduler (`JobScheduler` + `JobSubmissionService` remain the only
  durable machine-execution authority; watch reads hold no permits): pass —
  driver documented in `architecture/scheduler.md` + `ci_watch_idle.rs`.
- Singleton daemon (one notifier/watcher/driver per daemon): pass —
  `Arc::ptr_eq` test + construction wiring.
- Immutable in-flight snapshots / stable project identity: unchanged, pass.
- Notify-only default spends zero model tokens: pass — C001 starts no model
  turns; expiry/cancel never cancel remote workflows.
- Auth bound to immutable session/principal/workspace/repo: pass — exact-owner
  checks unchanged; lease owner is a service identity conferring no read rights.
- Credential refs only, URL allowlist/redirect/response caps, rate-limit/ETag
  preserved: pass — no transport change except claim gating.

## 6. Failure and recovery review

- Duplicate delivery: idempotent `ciwatch:<watch_id>` notice; 50-contender
  race yields one winner; late duplicate `AlreadyTerminal` with no re-poll.
- Cancellation races: `cancel` transitions + terminal notice; terminal targets
  never re-polled; user cancel wins (no autonomous turn in C001).
- Restart: `repair_registrations` heals insert-before-subscribe; fresh handles
  over the same pool see terminal watches + single notice; expired claims
  recoverable after `claim_until`; startup schedules without storm (bounded
  tick, backoff preserved).
- Partial persistence: subscribe failure leaves a repairable row, never a
  silent orphan; conflicting owner fails closed to attention.
- Stale generation: slow old holder's final update cannot overwrite a newer
  terminal (state CAS + DB-authoritative probe + lease release on terminal).
- Contention: per-watch mutex (fast-path) + DB lease (authority); global due
  bound 8/tick; fair due-order prevents starvation.
- Malformed/unauthorized: 401/403/404/429/5xx, ETag/Retry-After, malformed/
  large bodies, cross-origin redirects, guessed handles all fail closed.
- Bounded events: notice payloads bounded/redacted; counts bounded.

## 7. Migration and compatibility review

- Additive migration v76 only (`ALTER TABLE ci_watch ADD COLUMN` ×3 + claim
  index); no historical backfill beyond `NULL/NULL/0` (= pre-C001 unclaimed);
  `record_json` carries the new fields with serde defaults for old rows.
- `STORAGE_LAYOUT_VERSION` 75→76 tracks the highest migration
  (`check_project_catalog_invariants.py` passes via `verify.sh quick`).
- Rollback: dropping the three columns (or restoring v75 code) leaves v76
  rows readable (extra columns ignored); downgraded code treats leased rows
  as unclaimed (extra polls, no loss of terminal Notices — idempotent).
- M001/M002/M003/M004 suites green; `JobScheduler` canonical authority
  unchanged (only additive sync installer); `ci_watch` tool schema unchanged.

## 8. Security review

- Exact-owner preserved (`status`/`cancel`/`list`); stolen handles read as
  `NotFound` (no oracle); verified in C001 rate/security test.
- Idle driver observes with frozen watch lineage only; no principal
  substitution, no second-session wake, no archived-session reactivation.
- Credential refs only (`env:GITHUB_TOKEN`/`env:GH_TOKEN`/`none`); secrets
  never persisted/logged; `common::secret_scan` sentinels pass.
- Allowlist/redirect/ETag/rate-limit/TLS behavior unchanged; model surface
  carries no URL/token/callback (M004 tool test still green).
- Sandbox/approval ceilings untouched (no model execution in C001).

## 9. Documentation and operations

- `architecture/scheduler.md`: C001 timer/admission freeze (driver vs
  `ScheduleStore` vs `JobScheduler`, lease authority, singleton lifetime).
- `architecture/jobs.md`: idle coordinator + lease columns in watch section.
- `architecture/authorization.md`: C001 lease-owner non-principal note.
- `src/scheduler/ci_watch.rs` header: idle trigger is the driver, not
  turn-boundary-only.
- Operator: `watch_counts()` (active/overdue/leased/expired/attention/
  last-observed); driver `wake()` on creation/updates; shutdown cooperatively
  stops the loop (lease expiry recovers); startup repair logs
  `(repaired, attentioned)`.
- No new config keys; `CODEGG_GITHUB_API_ALLOWLIST` semantics unchanged.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Hosted `CI / verify` + Desktop E2E not run on exact head `21b35c79` | No hosted qualification for C001 | C003 must obtain exact-head hosted evidence before any merge-ready claim |
| low | Native macOS/Windows lifecycle not run; live GitHub API + paid provider not used | Platform/live-network qualification absent | Labelled unavailable per plan; no fabricated green; C003 carries the same condition |
| low | `cargo test` tail output did not re-capture M004/background exact counts (exit 0 observed) | Minor evidence-precision gap, no correctness risk | C003 re-runs all four suites with full counts |

No critical/high/medium findings remain. The stop conditions (interactive/
model-dependent progression, competing authoritative watchers, silent
unnotifiable watches, new independent scheduler) are all discharged.

## 11. Roadmap disposition

Milestone **closed**; hard successor **C002 may proceed to `ready`**.
C001's design exposes the minimal event/subscription interface C002 needs
(terminal M001 notice `ciwatch:<watch_id>` + subscription/lease rows) and
starts no model turns, as required.

## 12. Registry updates

- `plans/registry.md`: move C001 `ready` → `closed` (closure
  `plans/closure/async-background-jobs-corrective/001-status.md`,
  implementation `21b35c79`); move C002 `blocked` → `ready` (strict C001
  closure satisfied); C003 stays `blocked` on C002.
- `plans/subsystems/async-background-jobs-idle-completion-corrective-addendum.md`:
  status `active (C001 ready; C002/C003 blocked)` → `active (C001 closed; C002 ready; C003 blocked)`; milestone table C001 `ready` → `closed`.
- `plans/implementation/async-background-jobs-corrective/001-*.md`: `ready for handoff` → `implemented` (done in implementation commit).
- `plans/implementation/async-background-jobs-corrective/002-*.md`: `blocked` → `ready for handoff` (same closure commit per planning unblock rule).
