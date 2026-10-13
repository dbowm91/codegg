# Async Background Jobs M003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/async-background-jobs/003-safe-continuation-and-job-visibility.md`

Source subsystem roadmap:

- `plans/subsystems/async-background-jobs-and-ci-watch-roadmap.md#m003--safe-agent-continuation-attention-delivery-and-frontend-controls`

Repository baseline reviewed: `52d630fc9aae8460e8a4337ad76f622580b7568a`

Implementation commits or pull requests:

- `52d630fc` — feat(async-jobs): M003 safe continuation and job visibility (12 files, code + tests + docs)

## 1. Executive finding

M003's capability + invariant boundary is complete: when an async job finishes after its originating `AgentLoop::run()` has returned, the default is useful **notify-only** behavior (one durable visible notification, zero model tokens spent by delivery, inspectable on reconnect/restart/next turn), and an explicitly authorized bounded `resume_owner` policy yields exactly one continuation for one background wait at the next safe, user-authorized turn boundary. A live parent receives the result at the next safe boundary without duplicate provider entries; a dormant parent loses nothing and no inference is silently consumed. No infinite loops, no new chat runtime, no external messaging, no workflow engine, no user impersonation, no implicit wakeups for unrelated jobs.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| A. Delivery state diagram + CAS guards; ended-turn drop vs live loop; dual frontends + duplicate replay | `JobDeliveryState::{Pending, SessionEventCommitted, AvailableToOwner, ResumeQueued, Consumed, Expired, Suppressed}` + `delivery_state()`; `tests/job_continuation_m003_visibility.rs::{live_parent_gets_one_safe_boundary_notice, ended_turn_default_spends_no_tokens_keeps_durable_notice, explicit_resume_yields_exactly_one_continuation}` | pass | Claim/consume/suppress are CAS; duplicates + concurrent winners proven single |
| B. Smallest policy/record in session/turn notification domain; existing scheduler/turn runtime for new turn; atomic idempotency key from notice+lease; no new scheduler/direct model call; refuse unauthorized/expired/revoked/archived; conservative default; one-wait opt-in | `job_continuation_lease` (migration v74, `lease_id = notice_id = job_id`), `subscribe_with_options` (task-level auth), `consume_resume_lease` / `suppress_lease` CAS; `on_complete` on test/bash (default notify_only) | pass | Principal/workspace from daemon boundary only; model cannot escalate |
| C. Exact-owner safe-boundary wake; coordinate notifier/follow-up/provider/cancel/steer/completion; no synthetic utterance; no reopened turn; durable event available when live hint missed | `follow_up.rs` exact-session injection; `job_notification_recovery.rs` settle-then-inject; no `AgentLoop::run()` from scheduler callbacks; `cancel_expiry_and_foreign_owner_suppress_resume` | pass | Ended turn retains no stale sender; delivery waits for next authorized turn |
| D. Bounded job list + pending notices over existing projection; handle/kind/status/artifact/intent + cancel/wait/inspect; roles/redaction/reconnect/replay/out-of-order/multi-client; TUI shutdown; no TUI polling source-of-truth | `JobList` (existing) + `SessionEvent::JobTerminalNotification` (existing session projection) + `job` tool; `architecture/projection.md` visibility section; reconnect/restart agreement tests | pass | No new projection variant, no polling task |
| E. Foreground default; automatic notifications; resume-not-authorized hint; `job.wait` bounded manual observation; no poll scripts | Tool descriptions (`test`, `bash`, `job`) + `on_complete` param docs + resume marker text | pass | No cron/self-poll guidance anywhere |
| F. Docs (guarantees, budget/privilege boundary, resume controls, audit identity, recovery/stop) + registry/closure; M004 unblock only after positive tests | `architecture/agent.md`, `jobs.md`, `projection.md`; this closure; registry updates below | pass | M004 unblocked here |
| Acceptance: live parent one safe-boundary notice, no duplicate provider entry | `live_parent_gets_one_safe_boundary_notice` (1 message, 1 event, duplicates add none) | pass | — |
| Acceptance: ended default, no tokens, one durable visible notice on reconnect/restart/next turn | `ended_turn_default_spends_no_tokens_keeps_durable_notice` | pass | Fresh service + store views agree |
| Acceptance: explicit resume → one new bounded continuation, never another under duplicates/frontends, within bound/lease; cancel/archive/revocation/expiry wins | `explicit_resume_yields_exactly_one_continuation`, `cancel_expiry_and_foreign_owner_suppress_resume` (cancelled/expired/foreign → notify-only + suppressed, no marker) | pass | Single-wake budget, TTL, owner match enforced |
| Acceptance: crash at persist/append/ack/claim/turn-record/provider-start reconstructs; provider-start ambiguity → attention unless idempotent replay proven | `crash_boundaries_reconstruct_without_duplicate_inference` (append-crash confirms without re-inject; mark-crash consumes without second message) + confirm/ack CAS | pass | Three interleavings covered; no unbounded inference |
| Acceptance: failure/timeout/cancel/Interrupted classified; foreign receivers none; observer reads no secrets, cancels nothing | `failure_timeout_cancel_interrupted_classified_and_projection_agrees` + M001 owner tests re-verified | pass | `Interrupted` still never final (M001) |
| Acceptance: TUI + daemon agree on reconnect/recovery; no toast-only masquerade | Session-event count == terminal count; `delivery_state` terminal per notice; restart agreement test | pass | Durable receipt, not toast |

## 3. Production implementation evidence

- `src/scheduler/job_notifications.rs`: `JobCompletionDisposition` (notify_only default/resume_owner/suppress), `JobContinuationLease` + `JobContinuationStatus`, `JobContinuationPolicy` (30m default TTL, 4h max, 1 wake), `JobDeliveryState` diagram + `delivery_state()`, `subscribe_with_options` (lease creation at task-level authorization), `get_lease`/`consume_resume_lease` (CAS, owner + expiry + budget)/`suppress_lease`, lease persistence (`persist_lease`, `transition_lease`, `load_lease_from_pool`).
- `crates/codegg-core/src/session/schema.rs` + `storage/mod.rs`: migration v74 (`job_continuation_lease` + indexes), layout version 73→74, no backfill (absent lease = notify_only).
- `src/agent/job_notification_recovery.rs`: lease-aware loop — exact-session routing, confirm-then-claim/mark/settle/ack ordering, `settle_lease_for_delivered` (resume iff disposition + resumable status + valid lease + owner match; else suppress + attention), resume marker appended only on just-consumed lease, `resumed`/`resume_suppressed` accounting, `status_allows_resume` (completed/failed/timed_out only).
- `src/tool/{test.rs,bash.rs,bash/process.rs,job.rs}`: `on_complete` param (default notify_only), principal/workspace threaded from daemon boundary, disposition in handle JSON, poll-free + resume guidance in descriptions.
- `src/agent/{follow_up.rs,loop.rs,coordinator.rs,agent_loop_factory.rs,turn_runtime.rs}` + `tool/{factory.rs,mod.rs}`: M001 wiring reused (service install, registry, factory context); no new scheduler, no direct model call in handlers.
- Absent by design: autonomous idle wakeups, inference spend on delivery, cross-turn injection, reopened completed turns, synthetic user utterances, competing inference queues, external messaging, workflow engine.

## 4. Verification executed

### Commands run

```bash
cargo test --test job_continuation_m003_visibility
cargo test --lib follow_up
cargo test --lib run_control
cargo test --test tool_program_m016_notification_replay --test tool_program_m017_notification_confirmation --test scheduler_restart_recovery
cargo test --test headless_projection_consumer --test context_projection_adversarial
cargo test --test job_notification_m001_contract --test job_background_test_slice --test bash_background_m002_routes
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
scripts/verify.sh quick
bash scripts/check-core-boundary.sh
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
bash scripts/check_projection_disclosure.sh
bash scripts/check_projection_publication_seam.sh
python3 scripts/check_websocket_bounds.py
python3 scripts/check_authorization_matrix.py
```

### Results

- `job_continuation_m003_visibility`: 11 passed (6 new + 5 shared `common::secret_scan`), 0 failed.
- `--lib follow_up`: 0 tests (module has no unit tests; boundary covered by integration). `--lib run_control`: 6 passed.
- `tool_program_m016`: 2 passed. `tool_program_m017`: 2 passed (M017 guarantees retained).
- `scheduler_restart_recovery`: 15 passed.
- `headless_projection_consumer`: 2 passed. `context_projection_adversarial`: 90 passed.
- M001 suites: 17 + 10 passed. M002 suite: 12 passed.
- `cargo fmt --check`: pass. `clippy --workspace --all-targets -- -D warnings`: pass (same `--all-features` deviation as M001/M002: repo forbids it; one new `should_implement_trait` allow on `JobCompletionDisposition::from_str`) .
- `scripts/verify.sh quick`: **pass**.
- Guards: core-boundary pass, execution-ownership ok, scheduler-bypass ok, projection-disclosure OK, projection-publication-seam OK, websocket-bounds OK, authorization-matrix verified.
- Local only; no live-provider trajectories, no permanent new CI workflow (per plan); hosted CI / Desktop E2E not run (low findings).

## 5. Invariant review

- Three dispositions typed, `notify_only` default, `resume_owner` only after explicit bounded task-level authorization, `suppress` for cancelled/archived: schema enum + `on_complete` default + tests.
- Lease binds immutable session/owner/principal/workspace/turn ancestry, job/notice ID, mode, expiry, max wakes/budget; provider/model policy frozen at authorized boundary (no widening; principal/workspace daemon-resolved, model cannot supply): `subscribe_with_options` + lease row + test bindings.
- Exactly one accepted continuation per logical terminal (claim/started/finished or recoverable): `consume_resume_lease` CAS + duplicate/frontend tests + crash interleavings.
- Loss of sender ≠ loss of authority; append/ack/turn-start reconciled separately: three-step durable chain with confirm-then-mark, mark-then-consume, consume-then-ack; crash tests prove each.
- Exact owning turn/direct parent only; no cross-turn injection; no duplicate provider call while in flight (injection only at turn-start boundary into owning session messages); ended turn retains no stale sender: follow_up exact-session gate + foreign-session test (considered 0).
- `notify_only` persists redacted attention event for frontends; next turn inspects/consumes without duplicate insertion: append_idempotent + semantic confirm + ended-turn test.
- `resume_owner` queues through existing turn-runtime/JobSubmissionService path (authorization + lease), never recursive `run()` from scheduler callback, never a competing inference queue: handlers only mark/consume durable rows; the single continuation is delivered as a bounded marker at the next user-authorized turn.
- Deadline/expiry/denied/revoked/archived/unavailable-provider/full-mailbox/restart/exhausted-budget → explicit needs_attention/suppression, never hidden retry or charge: suppress paths + `resume_suppressed` accounting + tests.
- External job status untrusted; system message authenticated by job-store terminal state; no remote text as trusted instruction: `format_job_notice_content` from durable notice only.

## 6. Failure and recovery review

- Duplicate delivery/idempotency: notice first-write-wins + event idempotent append + lease CAS consume; duplicate terminals/concurrent frontends yield one message + one continuation (tests).
- Cancellation races: user cancel → terminal `cancelled` → lease suppressed, attention-only notice; cancel targets exact job + descendants (M001/M002 suites green).
- Daemon/node restart: SQLite leases/notices/subscriptions + `recover_from_store` + restart-agreement test (fresh views agree; pending survives).
- Partial persistence: notice/lease/event/ack steps independent; any crash reconstructs via confirm/mark/consume/ack without re-injecting or double-consuming (crash tests).
- Stale generation/lease: TTL expiry → `Expired`, never resurrected; re-consume fails; re-claim fails (tests).
- Contention/resource release: session bound + single-wake budget; no permits held by notice path; scheduler admission untouched.
- Malformed/unauthorized: empty session/key rejected; forged session fails closed; foreign session skipped with zero consideration; changed-payload conflicts (M001 green).
- Bounded event/artifact: 16/session notices, 2k summary, 1 wake/lease, 4h max TTL, 256-row recovery scan; provider-start ambiguity recorded as attention (suppressed + errors) unless idempotent replay proven (confirm path).

## 7. Migration and compatibility review

- Migration v74 additive (`IF NOT EXISTS` + indexes); no historical row modified; absent lease = notify_only (pre-M003 behavior). Layout version 73→74 tracks highest migration; `verify.sh quick` green.
- Backward compat: `on_complete` additive optional (default notify_only); M001/M002 notices without leases behave exactly as before (all M001/M002 suites green); session-event enum unchanged; no protocol change (existing `JobList` + session events + `job` tool).
- Config fallback: background without session/service still fails closed (M001/M002 tests green); resume without opt-in never resumes.
- Rollback: older builds ignore new table/param; new leases invisible (documented, acceptable).

## 8. Security review

- Lease authority: model cannot mint privilege beyond session contract (principal/workspace host-resolved, single wake, TTL cap, owner match, resumable-status gate); second principal/foreign session denied or skipped without leakage (tests).
- Secrets: redacted bounded summaries only; lease rows carry IDs/mode/expiry, never tokens/credentials; `secret_scan` shared tests pass; no secret logging.
- Path validation: workspace binding stored, never model-supplied; no new filesystem authority.
- Privilege boundary: no auto-inference spend; delivery spends zero tokens; continuation only at user-authorized turn with frozen policy; cancel/archive/revocation wins; authorization-matrix + disclosure + transport guards green.
- DoS bounds: 1 wake/lease, TTL caps, session notice bound, bounded scans; no polling tasks, no second scheduler, no unbounded provider work.

## 9. Documentation and operations

- `architecture/agent.md`: "Background job continuation (M003)" (dispositions, lease, routing, states, crash chain).
- `architecture/jobs.md`: "Safe continuation and visibility (M003)" (on_complete, lease bounds, states, pointers).
- `architecture/projection.md`: "Background job visibility (M003)" (existing surfaces, no new variant, no polling).
- Tool discovery: `test`/`bash` `on_complete` enums + `job` resume guidance; handle JSON carries disposition.
- `docs/execution-ownership.toml`: unchanged (no new owner; lease store is scheduler-domain definition). Guards green.
- Operator: `delivery_state()` queryable per notice; `resumed`/`resume_suppressed` in recovery reports; warn/debug logs on races; suppressed leases inspectable via `get_lease`; user cancel/steer invalidates outstanding resume (cancel → suppressed).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Hosted CI / Desktop E2E not run; evidence local `verify.sh quick` + focused suites | No hosted sweep proof on this SHA | Future hosted run may reference; no rework unless falsified |
| low | Native macOS/Windows not exercised | Platform-specific continuation/visibility unverified | Recorded honestly; M004 should include native evidence where available |
| low | No live-provider resume trajectory (no model turn consumed a resume marker) | End-to-end model behavior after resume authorization unproven with real inference | M004 or a live-turn qualification should include one real resume consumption; structural gates (single-consume, safe boundary, bounds) are proven |

No critical/high/medium findings. No corrective pass required. Non-idempotent inference restart ambiguity handled by `needs_attention` suppression paths, never blind replay.

## 11. Roadmap disposition

Milestone closed; next dependency may proceed:

- M004 (GitHub Actions remote CI terminal watcher) is **unblocked** (hard dependency M003 closed; soft dependency M002 already closed).
- No other registered plan lists M003 as a hard dependency.

## 12. Registry updates

- `plans/registry.md` Active subsystem roadmaps row: `active (M001–M002 closed; M003 ready; M004 blocked)` → `active (M001–M003 closed; M004 ready)`.
- `plans/registry.md` Dependency-ready plans: M003 row `ready` → `closed` (closure `plans/closure/async-background-jobs/003-status.md`, implementation `52d630fc`); add M004 row as `ready` (hard gate M003 closed, soft M002 closed; revalidate baseline before editing).
- `plans/registry.md` Blocked work: remove M004 blocked row.
- `plans/subsystems/async-background-jobs-and-ci-watch-roadmap.md`: M003 `ready` → closed; M004 `blocked on M003` → ready.
- `plans/implementation/async-background-jobs/003-*.md`: `ready for handoff` → `implemented`.
- `plans/implementation/async-background-jobs/004-*.md`: `blocked` → `ready for handoff` (M003 closure satisfies hard gate; revalidate baseline `52d630fc` before editing).
