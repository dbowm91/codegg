# Async Background Jobs M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/async-background-jobs/001-job-notification-contract-and-background-test.md`

Source subsystem roadmap:

- `plans/subsystems/async-background-jobs-and-ci-watch-roadmap.md#m001--generic-job-completion-contract-and-background-test-vertical-slice`

Repository baseline reviewed: `e828bebf940306992eec74aace0f41e114feced3`

Implementation commits or pull requests:

- `e828bebf` — feat(async-jobs): M001 job notification contract and background test slice (26 files, code + tests + docs; no closure/registry in that commit)

## 1. Executive finding

M001's capability boundary is complete: one end-to-end `test` invocation can submit a durable job, return a stable handle immediately, let the model do other work, and receive one actionable terminal notification without polling. The shared scheduler completion + session notification logic is factored for later Bash/CI consumers. Foreground `test` behavior is preserved. No auto-resume after idle turn, no generic workflows, no CI watching, no new executors — all explicitly out of scope and absent.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| A. Typed `JobHandle` + `JobTerminalNotice` DTO, stable ID, session/turn/lineage, status, RunStore link, bounded summary, timestamps, claim state | `src/scheduler/job_notifications.rs`: `JobHandle`, `JobTerminalNotice`, `JobBackgroundSubscription`; `notice_id = job_id`; `tests/job_notification_m001_contract.rs` | pass | Narrowly located in scheduler module; protocol reuse via existing `Job*` store + new session event (no new `CoreRequest` variant needed) |
| A. One minimal inspect/wait/cancel seam with same auth as `JobGet/JobWait/JobCancel` | `src/tool/job.rs` (`status\|wait\|cancel\|output`), exact-owner session scope; `tests/job_background_test_slice.rs::job_tool_enforces_exact_owner_and_wait_timeout_is_observation` | pass | `wait` timeout returns `still-running`, never failure |
| A. Fixtures: terminal-before-registration, same/different fingerprints, unauthorized cancel, timeout-as-observation, no re-execution | `tests/job_notification_m001_contract.rs::{terminal_before_subscription_healed_by_reconcile, changed_payload_duplicate_conflicts, forged_session_and_second_principal_denied_without_leakage}` + slice test wait/cancel | pass | Same key+same payload reuses job; same key+different payload conflicts (submission layer); wait timeout is observation |
| B. Reuse Tool Program claim/confirm/ack without changing TP IDs/tables | New `job_notification` + `job_background_subscription` tables (migration v73); TP `tool_program_notification` untouched; `src/agent/job_notification_recovery.rs` mirrors M017 CAS/append/mark/ack | pass | `tool_program_m016/m017` suites green |
| B. Narrow migration only if existing store cannot represent typed job origin | Migration v73 additive only; `STORAGE_LAYOUT_VERSION` 72→73; no backfill (absent subscription = foreground, pre-M001 behavior) | pass | `check_project_catalog_invariants` covered by `verify.sh quick` |
| B. Persist owner+receipt before wake; `append_idempotent` + semantic confirm | `record_terminal` persists notice before any delivery; `expected_job_event` + `EventStore::confirm_existing`/`append_idempotent` with `JobTerminalNotificationEvent::semantic_equals` | pass | `crates/codegg-core/src/session/{events,store}.rs` |
| B. Durable replay: crash before/after claim, after append/before ack, lost receiver, restarts, terminal-vs-retry races, stale claims, bounded backlog without oldest-N starvation | `reconcile_terminals`, `recover_from_store` (bounded 256, session-index served, backfill only), `expire_stale`→Expired (never→Pending), `enforce_session_bound` suppresses oldest; tests: duplicate idempotent, two concurrent claimants, stale expiry, sqlite restart, crash-after-append via confirm path | pass | One event per logical terminal; no per-job polling task |
| B. Invalid cross-session fails closed | `record_terminal` session-match guard + `check_owner`; `src/tool/job.rs::enforce_job_owner` (denied == not-found message) | pass | No data leakage |
| C. Consume scheduler terminal transition once via daemon scheduler | `JobScheduler::set_job_notification_service` + `notify_job_terminal` on executor completion, `mark_unschedulable`, queued `request_cancel`; best-effort (never fails job) | pass | Foreground/historic without subscription produce nothing |
| C. Terminal cancellation before attempt, run-store failure, completion-before-subscription | Queued-cancel hook; run-store failure path untouched (executor persists job regardless); completion-before-subscribe healed by `reconcile_terminals`/`recover_from_store` | pass | No forced model call on progress |
| D. `test.execution_mode=foreground\|background` (default foreground); split submit from wait; background awaits submission+subscription, returns handle, never blocks | `src/tool/test.rs`: `TestExecutionMode`, `submit_background`, schema enum, foreground path byte-preserved; `tests/job_background_test_slice.rs::{background_test_returns_handle_without_blocking_wait, background_test_without_session_fails_without_spawning}` | pass | Scope/stall/report caps, resource profile, workspace authority, audit attribution preserved |
| D. Atomic-or-reconciled subscription/submission; no claimed-ready handle until durable acceptance complete | `submit_background`: session+service checks before submit; submit with `SubmissionKey test-bg:<digest>`; `subscribe` before handle; crash-between healed by same-key retry | pass | No orphan accepted work after retry |
| E. Bounded status/wait/cancel/output; wait timeout = still-running; safe active-turn injection + next-turn recovery; exact-job cancel | `src/tool/job.rs`; `src/agent/job_notification_recovery.rs::inject_recoverable_job_notifications`; `src/agent/follow_up.rs` safe-boundary injection; cancel via `scheduler.request_cancel` (exact job + descendants, no global kill) | pass | No consumption-only toast; every notice is a durable session event + system message |
| F. Projection DTO/event if missing; docs in jobs/scheduler/authorization; catalog/discovery; ownership manifest; registry/closure | `SessionEvent::JobTerminalNotification`; `architecture/jobs.md`, `scheduler.md`, `authorization.md`; `job` tool auto-discoverable; `docs/execution-ownership.toml` (test reason updated, job entry added) | pass | No new execution owner (scheduler reuse) |

## 3. Production implementation evidence

- `src/scheduler/job_notifications.rs` (~900 lines): policy (16 pending/session, 5m lease, 8KiB payload, 2k summary), `JobHandle`/`JobTerminalNotice`/`JobBackgroundSubscription`/`RecoveredJobTerminal`, `JobNotificationService` (in-memory + SQLite CAS, first-write-wins, `is_final_job_state` excludes `Interrupted`, `terminal_status_str`, `bound_summary`, `job_handle_for`, `format_job_notice_content`, `injection_event_id_for`).
- `src/scheduler/scheduler.rs`: optional service field, `set_job_notification_service`, `notify_job_terminal` best-effort, hooks in executor-completion task, `mark_unschedulable`, queued `request_cancel`.
- `crates/codegg-core/src/session/{events.rs,store.rs,state.rs,schema.rs}` + `storage/mod.rs`: `JobTerminalNotificationEvent` + variant + tag + meta + semantic equality; store confirm/append support; TUI state no-op; migration v73 (`job_background_subscription`, `job_notification` + indexes); layout version 73.
- `src/tool/test.rs`: `TestExecutionMode`, `execution_mode` schema, `submit_background` (host session required, `test-bg:<digest>` key, subscribe-before-handle, no wait), foreground untouched.
- `src/tool/job.rs`: `JobTool` with `status|wait|cancel|output`, exact-owner enforcement, bounded output, no re-execution.
- `src/agent/job_notification_recovery.rs` + `follow_up.rs` + `coordinator.rs`/`loop.rs`/`agent_loop_factory.rs`/`turn_runtime.rs` + `tool/factory.rs`/`mod.rs`: durable claim→confirm→append→mark→ack loop, safe-boundary system-message injection, daemon wiring (scheduler install + registry + factory context).
- `src/permission/mod.rs`: `job` → `ShellExec` agreement.
- Absent by design: auto-resume engine, generic workflows, CI watching, new executors, second scheduler, polling loops, sandbox/approval weakening.

## 4. Verification executed

### Commands run

```bash
cargo test --test job_notification_m001_contract --test job_background_test_slice
cargo test --test tool_program_m016_notification_replay --test tool_program_m017_notification_confirmation --test scheduler_restart_recovery
cargo test -p codegg-core --lib session::
cargo test --lib job_notification
cargo fmt --all -- --check
bash scripts/check-core-boundary.sh
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
cargo clippy --workspace --all-targets -- -D warnings
scripts/verify.sh quick
```

### Results

- `job_notification_m001_contract`: 17 passed (12 new contract + 5 shared `common::secret_scan`), 0 failed.
- `job_background_test_slice`: 10 passed (5 new slice + 5 shared), 0 failed.
- `tool_program_m016_notification_replay`: 2 passed (M017 IDs/tables unregressed).
- `tool_program_m017_notification_confirmation`: 2 passed.
- `scheduler_restart_recovery`: 15 passed.
- `codegg-core --lib session::`: 72 passed.
- `--lib job_notification`: 9 passed (service unit + recovery); job tool unit covered in slice.
- `cargo fmt --all -- --check`: pass (after one `cargo fmt --all` for new test files).
- `check-core-boundary.sh`: pass. `check_execution_ownership.py`: ok. `check_scheduler_bypass.py`: ok.
- `cargo clippy --workspace --all-targets -- -D warnings`: pass. Note: plan asks `--all-features`; repo policy (`AGENTS.md`) forbids `--all-features` for workspace sweeps (drags real-server tests); `verify.sh quick` is the canonical clippy/check authority and it passed. Two clippy findings fixed in-tree (`should_implement_trait` allow on `TestExecutionMode::from_str`, `clone_on_copy` on `JobState` copy).
- `scripts/verify.sh quick`: **pass** (`Quick verification passed`), including agent schema, core/client/desktop boundaries, sandbox, execution-ownership, TUI authority, http-route-disposition, audit-coverage, scheduler-bypass, config-merge-coverage, no-hardcoded-models, provider wire/catalog/resilience, OpenAI endpoint, Eggwork routing, `cargo check --workspace --all-targets --locked`.
- Local only: hosted CI / Desktop E2E not run (see §10 low findings). No platform-limited failure waived; Linux-only paths exercised, Windows/macOS recorded as unavailable.

## 5. Invariant review

- One accepted call → one durable job (M009 key + fingerprint; same key/same payload reuses, same key/different conflicts, identical payloads distinct): enforced by `JobSubmissionService` (unchanged) + background `test-bg:<digest>` key; proven by `same_invocation_reuses_handle_different_payload_conflicts`.
- Caller cannot forge session/turn: background lineage comes only from `ToolExecutionContext`/tool binding, never model input; `subscribe` rejects empty session/key; cross-session mismatch fails closed (test).
- Job can complete before subscription: covered by `reconcile_terminals`/`recover_from_store` (test `terminal_before_subscription_healed_by_reconcile`).
- Notice only from persisted final outcome incl. failed/cancelled/timed-out/expired; `Interrupted` never final; running/progress/wait-timeout never terminal: `is_final_job_state` + tests.
- Notice stored before delivery, unique by job+owner+invocation, semantically deduped; live send is hint only; queue-full/ended-turn stays pending: CAS + session bound + recovery tests.
- No process re-execution on inbox failure: scheduler hook is best-effort log-only; recovery retries notice, never the job.
- Redacted bounded summary + RunStore refs, no dumps/secrets: 2k char bound + test; tool output prefers durable notice over re-execution.
- Scheduler fair admission + TestRunner contract + permissions preserved: foreground path untouched; background retains kind/profile/cwd/timeout/audit; `verify.sh quick` + scheduler suites green.
- Background requires real host session or fails without spawning: `submit_background` pre-submit checks + `background_test_without_session_fails_without_spawning` (job count unchanged).

## 6. Failure and recovery review

- Duplicate delivery/idempotency: `notice_id = job_id` first-write-wins; duplicate terminal same digest returns existing; different digest conflicts without overwrite; session-event `append_idempotent` + semantic confirm; two-listener CAS race yields exactly one winner (tests).
- Cancellation races: queued-cancel hook notifies terminal cancellation; running cancel terminalizes via executor path; `AlreadyTerminal` replays emit nothing (scheduler semantics preserved); job-tool cancel targets exact job + descendants.
- Daemon/node restart: SQLite durable subscription + notice; `recover_from_store` bounded rescan; fresh service recovers from same pool (test `sqlite_persistence_survives_service_restart`); Tool Program M016/M017 restart suites green.
- Partial persistence failure: notice failure logged, job unaffected; next-turn `recover_from_store`/reconcile heals; no silent loss (warn log + pending recovery).
- Stale generation/lease: `recover_expired`/`expire_stale` move `Claimed→Expired` terminally (never back to `Pending`); re-claim fails (test).
- Contention/resource release: session bound 16 suppresses oldest, keeps newest (test); no fixed oldest-N scan starvation (session index serves pending, store scan only backfills); scheduler permits untouched (notice path holds no executor permit).
- Malformed/unauthorized input: empty session/key rejected; forged session fails closed; guessed job id denied as not-found; changed-payload duplicate conflicts; all without leakage (tests).
- Bounded event/artifact: 2k summary, 8KiB payload policy, 16/session, 256-row recovery scan, 64-row session load; injection key `job-notify:<job>:<session>`, event id `job-event:<key>`.

## 7. Migration and compatibility review

- Migration v73 additive (`IF NOT EXISTS` tables + indexes); no historical job row modified or backfilled; absent subscription = foreground (pre-M001 behavior). `STORAGE_LAYOUT_VERSION` 72→73 tracks highest migration (project-catalog invariant preserved; `verify.sh quick` green).
- Backward compat: foreground `test` outputs unchanged (same submit-without-key + wait + format path); `test` schema additive optional `execution_mode` (default foreground); `job` tool is new discoverable surface, no existing tool renamed/removed; session-event enum additive (old JSON still deserializes; new variant no-ops in TUI state).
- Protocol negotiation: no `CoreRequest` change (reuse `JobGet/JobWait/JobCancel` authority); no storage rollback beyond standard SQLite downgrade caveat (new tables ignored by older builds, new notices invisible — acceptable, documented here).
- Legacy paths: direct raw-shell compatibility untouched (M002 scope); Tool Program M017 path untouched.

## 8. Security review

- Authorization: background lineage host-resolved; `job` tool exact-owner session equality on every action; denied == not-found message (no existence oracle); `permission` map `job→ShellExec` agrees with tool category.
- Secrets: notice summary bounded redacted runner report only; no command text beyond resolved argv-derived scope label, no tokens/URLs/credentials; `secret_scan` shared tests pass; never logged secrets.
- Path validation: workspace resolution via `workspace_id_for_root` (canonical); cwd from resolved test command, not model path; no new filesystem authority.
- Privilege boundary: background inherits foreground job kind/profile/timeout/audit; no broader capability, no alternate shell fallback, no sandbox downgrade; `verify.sh quick` sandbox + auth guards green.
- DoS bounds: 16 pending/session, 2k summary, 8KiB payload, 256 recovery scan, 5m claim lease, wait timeout clamped 1ms–300s (observation only); no per-job polling task, no second scheduler, no unbounded provider work.

## 9. Documentation and operations

- `architecture/jobs.md`: "Background jobs and terminal notices (M001)" (handle, subscription, final-only, job tool, owner rule).
- `architecture/scheduler.md`: "Generic job terminal notices (M001)" (bridge points, best-effort, recovery, v73 tables).
- `architecture/authorization.md`: "job exact-owner" scope rule.
- `docs/execution-ownership.toml`: `test` reason extended (background subscribe), `job` entry added (scheduler observation/cancel, no new owner).
- Tool catalog/discovery: `test` description + `execution_mode` enum; `job` tool registered with poll-free guidance; `ToolRegistry::with_defaults` exposes both (test asserts).
- Operator diagnostics: `tracing::warn` on notice persistence failure ("recovery will retry"); `recover_from_store` callable at turn/recovery boundaries; injection text is system-generated from durable terminal state.
- Static guards: core-boundary, execution-ownership, scheduler-bypass all pass; clippy `-D warnings` clean.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Hosted CI (`CI / verify`, Desktop E2E) not run; evidence is local `verify.sh quick` + focused suites | No hosted proof of the 12k-test sweep on this SHA | Future hosted run may reference this closure; no rework needed unless it falsifies |
| low | Native macOS/Windows execution not exercised; sandbox-required-unavailable and Windows shell routes untested | Platform-specific background behavior (process trees, required sandbox) unverified | M002 must cover Windows constrained mode + sandbox-required fail-closed with native evidence |
| low | No live-provider trajectory (no model tokens spent on background notices) | Safe-boundary injection proven by recovery unit/integration only, not a live model turn | M003 live-turn qualification should include one real turn-boundary injection |

No critical/high/medium findings. No corrective pass required.

## 11. Roadmap disposition

Milestone closed; next dependencies may proceed:

- M002 (background Bash/build/lint/format) is **unblocked** (hard dependency M001 closed; soft M003 not required).
- M003 (safe continuation/visibility) is **unblocked** (hard dependency M001 closed; may proceed against M001 test jobs, M002 soft only).
- M004 remains **blocked** on M003 (unchanged).

## 12. Registry updates

- `plans/registry.md` Active subsystem roadmaps row: `Async background jobs and CI watching` → `active (M001 closed; M002–M003 ready; M004 blocked)`; current milestone text updated.
- `plans/registry.md` Dependency-ready plans: M001 row `ready` → `closed` (closure `plans/closure/async-background-jobs/001-status.md`, implementation `e828bebf`).
- `plans/registry.md` Blocked work: remove M002 and M003 rows (now ready); add M002/M003 rows to dependency-ready section; keep M004 blocked on M003.
- `plans/subsystems/async-background-jobs-and-ci-watch-roadmap.md`: M001 `ready` → closed; M002/M003 `blocked on M001` → ready; M004 stays blocked on M003.
- `plans/implementation/async-background-jobs/001-*.md`: `ready for handoff` → `implemented` (closure accepted).
- `plans/implementation/async-background-jobs/002-*.md`, `003-*.md`: `blocked` → `ready for handoff` (M001 closure satisfies hard gate; revalidate baseline before editing).
