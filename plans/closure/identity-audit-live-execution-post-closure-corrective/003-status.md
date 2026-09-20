# Identity / Audit Live Execution Corrective M003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/identity-audit-live-execution-post-closure-corrective/003-scheduler-job-complete-live-audit.md`

Source subsystem roadmap:

- `plans/subsystems/identity-audit-live-execution-post-closure-corrective-addendum.md#M003 — Scheduler terminal job-completion audit`

Repository baseline reviewed: `591cb4de38fe6359371c1c9fb79c5538889ac58c`

Implementation commits:

- `591cb4de` — feat(identity): M003 scheduler job-complete live audit:
  `job_complete` at the durable scheduler terminal attempt transition
  (`persist_completion`, `mark_unschedulable`, queued `request_cancel`)
  through the M001 trusted context/emitter seam; durable
  `OriginAttribution` resolution with explicit `legacy-local` fallback,
  deterministic per-attempt event identity, bounded outcome labels,
  best-effort failure policy, M003 coverage-guard pins,
  `architecture/audit.md` (+ scheduler doc), 9-test regression matrix
  `tests/identity_m003_scheduler_job_complete.rs`.

## 1. Executive finding

M003 is closed. Real scheduler terminal transitions emit one structural
`job_complete` event with truthful outcome after the terminal state is
durably accepted. Success, failure, cancellation/interruption, timeout,
validation rejection, and queued pre-admission cancel each produce their
own bounded outcome label; retries keep the failed prior attempt
auditable and scope the successor separately; replays and restart
reconciliation of an already-terminal attempt emit no duplicate; legacy
rows without durable attribution fall back to explicit `legacy-local`
rather than a fabricated principal; audit store failure increments the
existing failure counters without corrupting terminalization.
`job_retry` remains the retry-request event, not a surrogate completion.
Scheduler authority/admission semantics are unchanged. No new
AuditAction, store, schema, or protocol was added. No ADR was required.

## 2. Requirement-to-evidence matrix

| Requirement (plan §) | Evidence | Result | Notes |
|---|---|---|---|
| Emit `job_complete` at the real durable terminal transition for success, failure, cancellation/interruption, other bounded outcomes (§1) | `persist_completion` returns the durably-accepted `JobRecord` and `emit_terminal_completion` runs only on `Ok` in `src/scheduler/scheduler.rs`; `mark_unschedulable` emits `failure` after `finish_attempt`; queued `request_cancel` emits `cancelled` only for the new `Cancelled` transition in `scheduler.rs`; `src/scheduler/job_complete_audit.rs` outcome map | pass | Ownership is the terminal write, never polling/observers/retry/completion consumers. |
| Canonical transition is the emission owner; no TUI/projection/retry/consumer emit (§2) | Only `scheduler.rs` terminal paths call `emit_terminal_completion`; no TUI, projection, `retry_job`, or `wait_for_completion` emit path; `check_scheduler_bypass` pass | pass | `wait_for_completion` still synthesizes a client-side projection only and emits nothing. |
| Attribution from persisted job/source/attempt + M001 seam; legacy fallback; never manufacture a human principal (§2) | `trusted_context_for_terminal_job` reads `OriginAttributionStore` scope `job` from the emitter pool, rebuilds via `AuthenticatedPrincipal::reconstructed` + gate-copied decision linkage, falls back to `TrustedExecutionAuditContext::legacy_local` on absent pool/row/lookup failure/legacy marker; `reconstructed` has no serde | pass | Tested with recorded attribution (truthful principal/correlation) and without (legacy-local decision, local-owner binding). |
| Emit after durable acceptance (§3) | Executor task matches on `persist_completion` `Ok(terminal_job)` before emit; `mark_unschedulable`/`request_cancel` emit after their terminal writes; persistence errors log without emitting | pass | Failed persistence emits nothing rather than a fabricated terminal. |
| One terminal transition → one deterministic event id (§3) | `terminal_scope` (`attempt:outcome`) + `terminal_scope_for_job` (`job:cancelled`) via `deterministic_event_id(decision, JobComplete, correlation, scope)` in `job_complete_audit.rs`; helper unit tests | pass | Distinct attempts/outcomes scope separately; same transition reuses the id. |
| Replayed completion/restart reconciliation emits no duplicate (§3) | Second `finish_attempt` on a terminal attempt fails `InvalidTransition` before any emit; recovery over a terminal job touches nothing; `ON CONFLICT DO NOTHING` returns the stored row; `replayed_completion_and_restart_emit_no_duplicate` test | pass | Recovery path adds no emit of its own. |
| Retry uses the next attempt; prior failed attempt remains terminal/auditable (§3) | `retry_job` eligibility unchanged; `retry_keeps_prior_attempt_and_scopes_next_attempt` shows failure row + distinct success row with distinct event ids | pass | No scheduler retry semantics changed. |
| Correlate project/session/turn/run/job/attempt where available (§3) | `chain_for_terminal_job` carries session/turn/job from the terminal `JobRecord` plus run from the completion; correlation from attribution (or job fallback); tests assert session/turn/job locators | pass | Project omitted: jobs are workspace-scoped and attribution rows carry no project; documented as where-available. |
| Metadata limited to ids, bounded labels, decision outcome; no payload/output (§3) | `job_complete_event` with `job.id`/`job.outcome`/`decision.outcome` + chain locators only; `assert_structural_only` on every matrix row (no `echo m003`, no body/content digest) | pass | Executor summaries stay in the job store, never in audit. |
| Full regression matrix (§4) | `tests/identity_m003_scheduler_job_complete.rs` (9 tests) + unit suites below | pass | See §4. |
| Scheduler authority/admission unchanged (§6) | `check_scheduler_bypass` pass; admission/queue/dispatch code untouched except additive audit emit; `scheduler` lib suite green | pass | `docs/execution-ownership.toml` needed no change (audit emission is not execution). |
| `job_retry` stays a retry action, not a surrogate completion (§6) | `INSTRUMENTED_OPERATIONS` mapping retained; coverage causation now reads terminal-live + retry-as-request; daemon `job_retry` handler untouched | pass | No duplicate terminal: retry request (`retry` outcome) and terminal (`success`/`failure`/…) scope different ids. |

## 3. Production implementation evidence

- `src/scheduler/job_complete_audit.rs` (new, ~230 lines):
  `SCHEDULER_TERMINAL_CLIENT`, `job_outcome_label`,
  `job_outcome_for_attempt_state`, `terminal_scope`,
  `terminal_scope_for_job`, `chain_for_terminal_job`,
  `trusted_context_for_terminal_job` (durable attribution → reconstructed
  principal or `legacy_local`), `emit_job_complete` (deterministic id +
  `job_complete_event` with `allow`), `emit_terminal_completion`, 3 unit
  tests.
- `src/scheduler/scheduler.rs`: executor task snapshots the injected
  emitter and emits only after `persist_completion` returns the
  durably-accepted `JobRecord`; `persist_completion` now returns
  `JobRecord` instead of `()`; `mark_unschedulable` emits `failure`
  after its terminal write; queued `request_cancel` emits `cancelled`
  only for the new `Cancelled` transition (`Requested` terminalizes
  later via the executor path; `AlreadyTerminal` emits nothing).
- `src/scheduler/mod.rs`: exposes `job_complete_audit`.
- `crates/codegg-core/src/transport_auth.rs`:
  `AuthenticatedPrincipal::reconstructed` — lossless restoration from
  durable attribution fields (no serde, scheduler-only caller).
- `crates/codegg-core/src/audit_instrumentation.rs`:
  `ExecutionAuditEmitter::pool_snapshot` for the attribution lookup;
  `job_complete` coverage causation updated to terminal-live +
  retry-as-request (mapping retained, `live_mapped` stays true).
- `scripts/check_audit_coverage.py`: new 8th check
  `check_m003_scheduler_job_complete_present` pinning every hook site
  (8/8 green).
- Docs: `architecture/audit.md` gap list narrowed (distributed actions
  remain future) + new "Scheduler terminal hook (M003)" section;
  `architecture/scheduler.md` emitter ownership updated.
- Tests: `tests/identity_m003_scheduler_job_complete.rs` (9 tests).

Distinguished as absent by design (not defects):

- No new `AuditAction`, store, schema, migration, protocol version bump,
  or public SDK API.
- Recovery (`recover_generation`) adds no emit of its own; terminal
  jobs recovered as terminal stay single-row, requeued jobs emit at
  their later real terminal.
- Project locator omitted on terminal events (workspace-scoped jobs;
  attribution rows carry no project); session/turn/run/job locators
  included where the persisted record provides them.
- `wait_for_completion` terminal synthesis remains a client projection
  and emits nothing.
- Distributed `node_enrollment` / `remote_execute` remain future scope.

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg --lib scheduler -- --test-threads=1
cargo test --workspace job --no-fail-fast
cargo test --test identity_m005_audit_instrumentation -- --test-threads=1
cargo test --test identity_m003_scheduler_job_complete -- --test-threads=1
cargo test --test identity_m002_live_audit_hooks -- --test-threads=1
cargo test -p codegg --lib job_complete_audit -- --test-threads=1
python3 scripts/check_scheduler_bypass.py --self-test
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_audit_coverage.py --verbose
python3 scripts/check_execution_ownership.py
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
```

### Results

- `codegg --lib scheduler`: pass, 81 passed (no scheduler regression;
  new emitter threading is additive).
- `cargo test --workspace job`: pass, no failures (all `job`-filtered
  suites green).
- `identity_m005_audit_instrumentation`: pass, 13 passed (no
  builder/fixture regression).
- `identity_m003_scheduler_job_complete`: pass, 9 passed (success;
  failure; cancelled/interrupted/timed_out distinguishability; retry
  prior + successor; replay + recovery no-dup; queued-cancel terminal +
  replay silence; legacy fallback; audit-failure terminalization +
  counter; deterministic helper).
- `identity_m002_live_audit_hooks`: pass, 17 passed (no command/Git/
  interactive regression).
- `codegg --lib job_complete_audit`: pass, 3 passed (bounded outcomes,
  scope separation, no invented success).
- `check_scheduler_bypass.py --self-test`: pass.
- `check_scheduler_bypass.py`: pass.
- `check_audit_coverage.py --verbose`: pass, 8/8 (7 pre-existing plus
  the new M003 terminal-hook check).
- `check_execution_ownership.py`: pass (no new spawn surface; audit
  emission is not execution).
- `scripts/verify.sh quick`: pass (`==> Quick verification passed.`;
  covers fmt, agent schema, core-boundary, sandbox,
  execution-ownership, tui-authority, http-route-disposition,
  audit-coverage, scheduler-bypass, workspace check).
- `cargo clippy --workspace --all-targets --locked -- -D warnings`:
  pass, 0 warnings.
- `git diff --check`: pass.

Terminal transition matrix (all via the new hook, queried as
`job_complete` by job id):

| Terminal path | Outcome label | Rows | Replay |
|---|---|---|---|
| Executor `Completed` | `success` | 1 | second `finish_attempt` fails, still 1 |
| Executor `Failed` | `failure` | 1 | — |
| Executor `Cancelled` | `cancelled` | 1 | — |
| Executor `Interrupted` | `interrupted` | 1 | — |
| Executor `TimedOut` | `timed_out` | 1 | — |
| `mark_unschedulable` validation | `failure` | covered by owner (no separate live executor in matrix; helper pins the label) | — |
| Queued `request_cancel` | `cancelled` | 1 | `AlreadyTerminal` replay, still 1 |
| Retry prior `Failed` + successor `Completed` | `failure` + `success` | 2 distinct ids | prior row unchanged |

Duplicate/restart evidence: `replayed_completion_and_restart_emit_no_duplicate`
finishes one success terminal (1 row), fails the replayed
`finish_attempt` with `InvalidTransition` (still 1 row), runs
`recover_generation` over the terminal job (still 1 row).
Attribution fallback evidence:
`legacy_row_without_attribution_uses_explicit_fallback` emits once with
`decision_id = legacy-local`, `actor_principal = local-owner`, truthful
`success` outcome, structural-only metadata. Attributed rows carry the
recorded decision/correlation and session/turn/job locators.
Audit-failure evidence:
`audit_store_failure_keeps_terminalization_and_counts_failure` closes
the audit pool, still reaches executor `Completed` and a terminal job
state, and observes `emit_counters_snapshot().failed` growth.

All evidence above is local execution and is labeled accordingly. No
hosted `CI / verify` run is attached; routine CI remains the operator
action per the closed development-verification roadmap.

## 5. Invariant review

- Actor/provenance derives from durable admitted attribution, never
  caller/model strings: the hook reads the first-write-wins
  `OriginAttribution` row and rebuilds via `reconstructed`; payloads,
  `principal_ref`, tool inputs, model outputs, and grant strings cannot
  supply the context (no serde on the context or principal).
- Coordinator remains sequence/store authority: every hook emits
  through `ExecutionAuditEmitter` (one bounded timeout, existing
  counters/warn); no second database, bus, or queue; deterministic ids
  reuse stored rows via `ON CONFLICT DO NOTHING`.
- No command/payload/output/secret content in audit state: metadata is
  `job.id`/`job.outcome`/`decision.outcome` plus chain locators;
  per-row structural negatives assert the absence; the store secret
  guard still rejects secret-bearing keys/values/bodies.
- One transition emits at most one event: each terminal owner emits
  once after durable acceptance; the retry request (`retry` outcome)
  and the terminal (`success`/`failure`/…) scope different ids;
  replays fail before emission or dedupe by id (tested).
- Bounded best-effort failure: emitter timeouts/failures increment
  `failed` with warn logs and never fail terminalization (tested with
  a closed pool).
- Execution ownership not bypassed: dispatch/admission/queue code
  unchanged except additive emit; `JobSubmissionService` remains the
  creation boundary; execution-ownership and scheduler-bypass guards
  pass; `docs/execution-ownership.toml` unchanged.
- Personal-local and team execution share one composition: same
  context/emitter/hook types; unattributed rows use explicit
  `legacy_local` rather than inventing identity.

## 6. Failure and recovery review

- Pool-less daemon: emitter without a pool drops with
  `dropped_no_pool` (M001-tested); the hook resolves the legacy
  context and emits through the same drop path; execution unaffected.
- Store failure / timeout: `failed` counter increments with warn; the
  terminal job state is already durably accepted before the emit, so
  the job terminalizes on its own terms; audit never changes control
  flow (tested).
- Retry/replay: deterministic ids per (decision, action, correlation,
  attempt:outcome) return stored rows; tested for executor replays,
  queued-cancel replays, and recovery over terminal jobs.
- Cancellation races: pre-spawn cancels complete as `Cancelled`
  through the executor path (one event); queued cancels complete via
  `request_cancel` (one event); running-cancel requests emit nothing
  until the executor terminal arrives (no double).
- Malformed/unauthorized input: validation rejections terminalize as
  bounded `failure` via `mark_unschedulable` (one event); unknown
  executors follow the same owner; no fabricated success.
- Restart: `recover_generation` adds no emit; terminal jobs stay
  single-row; requeued jobs emit at their later real terminal.

## 7. Migration and compatibility review

- No storage migration: no schema change; catalog layout unchanged.
- No protocol change: no new `CoreRequest`/`CoreResponse` variants, no
  `PROTOCOL_VERSION` bump; the new context accessor and principal
  constructor have no `serde` impls and never appear in DTOs.
- Backward compatible: `persist_completion` return-type change is
  private to the scheduler; all new core APIs are additive;
  scheduler/emitter fields keep `None` defaults; existing suites green.
- Rollback: dropping the M003 commit restores silent scheduler
  terminals; rows already written remain ordinary queryable audit rows.

## 8. Security review

- No new authority: the emitter performs no authorization decisions;
  the hook fires only after the owning terminal transition is
  durably accepted.
- No principal fabrication: contexts come from the durable admitted
  row or explicit `legacy_local`; `reconstructed` restores stored
  fields only and is unreachable from wire input.
- No wire injection: no `serde` on contexts/principals/emitters; DTOs
  cannot supply them.
- Secrets: ids and bounded labels only; executor summaries, payloads,
  and outputs never enter metadata; existing secret-negative suites
  still green plus new per-row negatives.
- Denial-of-service bounds: one event per terminal transition, no
  unbounded queues, no new network/spawn surface; counters expose
  pressure.
- `codegg-core` boundary guard passes (new core APIs add no
  `crate::authorization` import to instrumentation production code;
  the scheduler lives in the root crate and consumes core types only).

## 9. Documentation and operations

Updated:

- `architecture/audit.md` — gap list narrowed to distributed actions;
  new "Scheduler terminal hook (M003)" section (ownership,
  attribution, correlation, idempotency, failure policy, guard/test
  pointers).
- `architecture/scheduler.md` — emitter ownership now M001/M003 with
  the three terminal owners named.
- `scripts/check_audit_coverage.py` — executable M003 hook pins (8/8).
- `plans/implementation/identity-audit-live-execution-post-closure-corrective/003-scheduler-job-complete-live-audit.md`
  — marked closed, linking this record.
- `plans/implementation/identity-audit-live-execution-post-closure-corrective/004-live-execution-audit-qualification.md`
  — marked ready (hard deps M002+M003 now closed).
- `plans/subsystems/identity-audit-live-execution-post-closure-corrective-addendum.md`
  — M003 `ready` → `closed`; M004 `blocked` → `ready`.
- `plans/registry.md` — subsystem row, dependency-ready table,
  blocked work, execution-order gate, closure-evidence table (see §12).

Operator note: no migration, no config change. `appended` will grow by
one row per scheduler terminal (success/failure/cancelled/timed_out/
interrupted/validation/queued-cancel). `failed` growth still means
audit I/O saturation — shed load rather than retrying with fresh ids.

## 10. Unresolved findings

There are no unresolved critical, high, medium, or low M003 findings.

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| — | — | — | — |

Explicit non-claims with named consumers (not defects):

- Guard/trajectory qualification remains M004 scope after M003.
- Production turn/job attribution threading beyond the durable job row
  (live context propagation at turn/job boundaries) rides with M004,
  not this hook milestone.
- Recovery-terminal jobs that never pass through `finish_attempt`
  stay single-row via the no-emit recovery path; their next real
  terminal emits normally.
- Distributed `node_enrollment` / `remote_execute` remain future scope.

No stop condition fired: sequence/store authority stays with the
coordinator; no second audit database or event bus; no trusted
principal/provenance in public untrusted DTOs; no authorization-model
change.

## 11. Roadmap disposition

Milestone closed and downstream work unblocks:

- M003 `ready` → `closed`.
- M004 (qualification and coverage-guard closure) `blocked` → `ready`:
  both hard dependencies (M002 + M003) are now closed with stable
  contracts (`TrustedExecutionAuditContext`, `ExecutionAuditEmitter`,
  command/Git/scheduler terminal hooks). No other hard dependency
  remains.
- No corrective pass is required.

## 12. Registry updates

Included in the closure commit alongside this record:

- M003 source plan marked closed, linking this record.
- M004 source plan marked ready (hard deps M002+M003 satisfied).
- Roadmap milestone table: M003 `ready` → `closed` with closure link;
  M004 `blocked` → `ready`.
- Registry active-subsystem row: Identity current milestone `M001+M002
  closed; M003 ready; M004 blocked` → `M001+M002+M003 closed; M004
  ready`; blocker column cleared to the remaining qualification scope.
- Registry dependency-ready table: M003 row `ready` → `closed` with
  closure link and implementation `591cb4de`; M004 row added as `ready`
  with hard dependencies noted satisfied.
- Registry execution-order gate: post-closure cleanup gate advanced
  from "M001+M002 closed; M003 ready; M004 blocked on M003" to
  "M001+M002+M003 closed; M004 ready".
- Registry blocked-work table: M004 blocked row removed (now ready).
- Registry closure-evidence table: Identity M003 row added pointing at
  this record with implementation `591cb4de`.
