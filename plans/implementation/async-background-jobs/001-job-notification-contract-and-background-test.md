# Async Background Jobs M001 — Job Notifications and Background Test Vertical Slice

Status: ready for handoff
Baseline: `e6a5e85de632ea5057d59aee585942d10f9d950b` (revalidate implementation branch before editing)
Roadmap: `plans/subsystems/async-background-jobs-and-ci-watch-roadmap.md#m001--generic-job-completion-contract-and-background-test-vertical-slice`
Class: capability + invariant. Hard dependencies: existing scheduler, Tool Program M017/M020 and AgentRun M009 closures, satisfied. Interface dependency: current `CoreRequest` job/projection/authorization contracts (inspect live code first).

## Objective and non-goals

Deliver one end-to-end `test` invocation that can submit a durable job, return a stable handle immediately, let the model do other work, and receive an actionable terminal notification without polling. Factor scheduler completion and session notification logic for later Bash/CI consumers. Preserve current `test` foreground behavior byte-for-byte where practical. Do not implement auto-resume after idle turn, generic workflows, CI API watching, or new executors.

## Source evidence and ownership

- `src/tool/test.rs`: `run_scheduled_test()` submits `NewJob { kind: Test }` then always `wait_for_completion`; test tool schema has no execution mode.
- `src/tool/tool_program.rs`: known-good `execution_mode=background`, `ProgramHandle`, accepted-call identity.
- `src/scheduler/tool_program_notifications.rs`, `src/agent/tool_program_recovery.rs`: durable Tool Program notification CAS/append/mark/ack and crash recovery. Reuse verified mechanics; do not rewrite the M017 state machine casually.
- `src/agent/loop.rs` / `src/agent/follow_up.rs`: safe-boundary injection and nonblocking follow-up channel.
- `src/agent/turn_runtime.rs`: service creation and live turn registration.
- `src/scheduler/job_complete_audit.rs`, `src/scheduler/scheduler.rs`, `src/core/daemon_jobs.rs`, `crates/codegg-core/src/jobs/{mod,store}.rs`, `crates/codegg-protocol/src/core.rs`: canonical terminal event, durable job, protocol.

## Invariants, failure semantics

One accepted model call -> one durable job, keyed by M009 caller/turn/invocation identity; replay same key/same fingerprint returns same handle, same key/different payload yields conflict, two calls with identical payload create distinct jobs. Authorized caller cannot choose untrusted session/turn IDs. A job can complete before notification subscription. Notification creation only from persisted *final* outcome, including failed/cancelled/timed-out/expired terminal states as applicable; `Interrupted` with pending recovery is not declared final. Running/progress and bounded observation timeout are not terminal outcomes.

Notification is stored before delivery, uniquely identified by job + subscribed owner/invocation and semantically checked for duplication. Live bounded send is a hint, not the durable receipt. If queue full or turn ended, keep notice pending for M003/next-turn recovery. Do not retry process execution because projection/inbox insertion failed. Expose redacted bounded status/summary and RunStore references; no stdout dumps/credentials in the notice. Preserve scheduler fair admission and existing TestRunner contract and permissions. Background mode requires a real host-originated session context or explicitly fails without spawning.

## Required work packages (ordered)

A. **Freeze contract/tests.** Define typed `JobHandle` and `JobTerminalNotice` or equivalent narrowly located protocol/core DTO, with stable ID, session/turn/lineage, status, RunStore link, bounded summary, timestamps and claim state. Document whether `job` control is a new discoverable model tool or existing tool action; choose one minimal inspect/wait/cancel seam with same auth as `CoreRequest::JobGet/JobWait/JobCancel`. Focused fixtures: terminal-before-registration, same/different invocation fingerprints, unauthorized status/cancel, timeout-as-observation, no process re-execution.

B. **Persisted notification bridge.** Factor reusable claim/confirmation/ack mechanism from Tool Program notification/recovery *without* changing existing Tool Program IDs/tables or breaking M017. Add narrow schema migration for job-notification table/index only if existing store cannot represent typed job origin safely; record rationale. Persist owner and receipt before wake, and use session-event `append_idempotent` plus semantic confirmation. Durable replay must handle crash before/after claim, after event append/before ack, lost live receiver, restarts, terminal-versus-retry races, stale claims and bounded backlog without a fixed oldest-N scan starving new notices. Insert one event per logical terminal; no per-job polling task. Invalid cross-session messages fail closed.

C. **Completion integration.** Consume existing scheduler terminal transition / event bridge once, ideally through the same daemon-owned scheduler service. A notification is recorded for *explicit* subscribed background jobs only; ordinary foreground or historic jobs must not retroactively create action items. Ensure terminal cancellation before attempt, run-store persistence failure and job completion before subscription are covered. No forced model call on progress.

D. **Test vertical slice.** Extend `src/tool/test.rs` with `execution_mode: foreground|background` (default foreground). Split common validate/policy/build/submit from foreground wait/format. Background path awaits durable submission and notification registration, returns stable `JobHandle`/inspect/cancel refs, and never calls the blocking wait. Preserve test scope, stall timeout, report capping, resource profile, workspace authority, goal correlation, and observation/audit attribution. Foreground remains unchanged. If notification registration and submission cannot be atomic, use a durable subscription/submission intent or restart reconciler: crash between them must not orphan accepted background work. Do not return a claimed-ready handle until the durable acceptance path is complete.

E. **Model control and safe injection.** Expose bounded job status/wait/cancel/output retrieval through one authorized model tool/protocol adapter; `wait` timeout means still-running. Inject terminal notice at a safe active-turn boundary and recover pending notices at the next turn. No consumption-only log toast. Background job cancellation must cancel the exact accepted job and descendants through scheduler, never an unrelated worktree/job.

F. **Visibility and documentation.** Add projection DTO/event only if missing existing `Job*` events cannot carry the handle/status contract. Document API, job vs AgentRun vs ToolProgram identity, error classifications and legacy foreground preservation in `architecture/jobs.md`, `architecture/scheduler.md`, `architecture/authorization.md` as affected. Update tool catalog/discovery guidance, static execution ownership manifest only for genuinely new owners, registry/closure.

## Tests and exit criteria

- Deterministic local test executor remains running while parent executes a second model tool; it then terminates and the parent sees one completion notice without calling `status` repeatedly.
- Completion before return/subscribe, cancellation before executor start, failure, deadline timeout, restart, and output overflow each produce the correct stable notice; retryable `Interrupted` does not emit false final.
- Exact-once *logical* session insertion under duplicate terminal event, two concurrent frontend listeners, and failpoint after append/before ack; original Tool Program M016/M017 suites remain green.
- Forged session/turn, guessed job ID, second principal, and changed-payload duplicate are denied/conflicted without data leakage.
- Foreground test outputs remain compatible and no new synchronous wait introduced into the background path.
- Run focused `cargo test --test tool_program_m016_notification_replay --test tool_program_m017_notification_confirmation --test scheduler_restart_recovery`, focused new notification/test trajectory suites, `cargo fmt --all -- --check`, core scheduler/authorization guards, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `scripts/verify.sh quick`. A platform-limited failure must be recorded, not waived.
- **Stop** if the implementation requires an additional scheduler, a generic auto-resume engine, weakening approval/sandbox, or cannot prove submission/notice crash recovery. Record a corrective instead of asserting closure.

Closure: `plans/closure/async-background-jobs/001-status.md`; only after strict closure mark M002/M003 ready.
