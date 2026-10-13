# Asynchronous Background Jobs and External Completion Watching

Status: active — M001 closed; M002–M003 ready; M004 blocked on M003

Planning baseline: `e6a5e85de632ea5057d59aee585942d10f9d950b` (default-branch source inspected 2026-10-13). Revalidate the actual implementation head before any code change.

Normative sources:
- `plans/000-long-term-specification.md` §§16–18, 26, 29 (run hierarchy, jobs, recovery, invariants)
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md` (durable jobs/scheduling and agent hierarchy)
- `plans/003-planning-process.md`
- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`
- `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`

Related closed authorities: `plans/subsystems/agent-run-worktree-concurrency-roadmap.md` (M005/M009), `plans/subsystems/tool-programs-roadmap.md` (M008/M017/M020), `architecture/jobs.md`, `architecture/scheduler.md`, `architecture/tool_programs.md`, `architecture/authorization.md`, `architecture/process-tool-execution-ownership.md`.

## Purpose, outcome, and boundary

Let an agent submit long-running work, receive a stable job handle without blocking on a tool call, continue useful work, and receive an actionable *terminal* completion without authoring shell-based or model-driven polling loops. Support ordinary tests and builds first; add explicit and authorized idle continuation, then GitHub Actions run/check watching. Default synchronous tools stay intact.

The sole machine-work admission and lifecycle authority is the existing daemon `JobSubmissionService` / `JobScheduler` / `JobStore`. Existing `ManagedProcessService`, `TestRunner`, `RunStore`, `ToolBroker`, sandbox and approval boundaries remain canonical. A background handle is a view of an existing durable `JobId`, **not** an unowned Tokio task, child shell `&`, generic workflow DAG, new scheduler, or agent-created polling script.

Ownership distinctions: `Job` is scheduling/attempt authority; `RunStore` stores execution/artifacts; `AgentRun` and `AgentRunGroup` are delegated-agent domains; `ToolProgram` owns deterministic programmable execution; session events and notifications are delivery/evidence. Do not flatten them or modify their historical closure records.

## Verified starting state and gaps

1. `src/tool/test.rs` already submits scheduler jobs but unconditionally calls `wait_for_completion`. `src/tool/bash/process.rs` submits routed test/build/lint/managed/shell jobs and awaits them. `src/tool/bash.rs` exposes command/workdir/timeout but no background mode. Direct raw-shell compatibility still exists and must not bypass policy for a background request.
2. `src/tool/tool_program.rs` already exposes `execution_mode: foreground|background` and a `ProgramHandle`. `src/scheduler/tool_program_notifications.rs` plus `src/agent/tool_program_recovery.rs` implement durable, idempotent session-event recovery for Tool Programs; do not regress this path.
3. `src/agent/run_control.rs` sends top-level child completion to the exact *live* `(session_id, turn_id)` and nested children to the direct parent. Terminal state persists if the receiver no longer exists. `src/agent/follow_up.rs` uses nonblocking `try_recv`; `AgentLoop::run()` does not process messages arriving after it returns.
4. `CoreRequest::JobSubmit/JobGet/JobWait/JobCancel/JobAttempts` and scheduler/projection events already exist. `JobWait` is an observation with a bounded timeout, not a detached process or an automatic agent wakeup.
5. No verified model-facing GitHub Actions completion watch exists. CLI shell loops are not an acceptable replacement.
6. Existing background work includes unrelated TUI task lifecycles and recurring `ScheduleStore` tasks; these are **not** the proposed agent job completion contract.

## Required invariants

- Exactly one canonical scheduler job per accepted tool invocation; retry of the same accepted invocation reuses the same job. Distinct invocations, even with identical commands, remain distinct. M009 accepted invocation identity and fingerprint-conflict rules apply.
- Submission returns a handle only *after* durable acceptance. A timeout while **waiting** never kills the job; an execution deadline may terminate it under existing process-tree supervision.
- The job inherits authorization, tool contract, configured sandbox, workspace/root, execution target, environment, approver decision, timeout, audit lineage, and resource profile from the foreground operation. Background execution creates **no** broader capability or alternate shell fallback.
- Status/report/inspect/cancel enforce the authenticated actor's project/workspace/session scope; knowledge of a `JobId` alone confers no rights. Never allow a model to forge principal/session/turn fields or inject an untrusted completion event.
- Terminal notification identity is deterministic and bounded. Notification data carries status, job/run/artifact references and redacted summaries—not command text, secrets, tokens, arbitrary URLs or unbounded logs. Progress updates do not cause model turns.
- Persist-before-notify and claim/ack/replay prevent notification loss or duplicate actionable delivery across daemon restart, channel overflow, reconnection, completion-before-subscription, cancellation races, and provider retries. Clarify logical exactly-once delivery rather than promise exactly-once network transmission.
- Job `Interrupted` / retry transitions must be reconciled against authoritative final job state: do not announce final failure while an eligible attempt may be requeued. Terminal outcomes remain immutable once truly final.
- No automatic model resumption after an idle turn by default. An explicit continuation lease/policy is required to spend inference or perform subsequent tools. Notifications remain visible without resumption.
- Per-session outstanding handles, notification count/bytes, active watchers, retry/poll budget, wake concurrency, and continuation count are bounded. No per-job busy-polling loop, duplicate scheduler, permanent new CI lane, or unbounded provider requests.

## Minimal cross-milestone contract

`execution_mode = foreground | background` on eligible model tools; foreground preserves current results and semantics. Background returns a typed bounded `JobHandle { job_id, kind, status, submitted_at, inspect_ref, cancel_ref, optional run_id }`. Inspect/wait/cancel may reuse existing daemon Job requests through a narrowly authorized model tool (e.g. `job` with `status|wait|cancel|output` actions) or the smallest existing tool surface; do not make the model poll. A new name/tool requires tool-surface/discovery policy review. Retrieval of full output uses bounded RunStore artifact handles. Notification envelope must include job ID, owning session/turn or run lineage, disposition, stable event ID, delivery state, and minimal result references.

Delivery tiers: (a) persist terminal notification, (b) if owning turn/run is still active, signal a safe boundary, (c) if no owner is active, leave a durable inbox item and optionally resume only under an explicit bounded continuation authorization. Do not let delivery-channel success substitute for stored session-event confirmation.

## Dependency graph and handoff order

```text
existing scheduler + tool-program notification + agent-run M009 (closed)
   |
   +-- M001  shared job notification contract + background test vertical slice  [ready]
          |
          +-- M002  background bash/build/lint/format, canonical policy parity [blocked: M001]
          |
          +-- M003  idle continuation policy, safe wake, projections/TUI       [blocked: M001]
                 |
                 +-- M004 GitHub Actions external watch + completion handoff   [blocked: M003]
```

M002 and M003 may proceed independently once M001 closes; M004 has a soft dependency on M002 for general tool affordances but a hard dependency on M003 for durable completion/continuation policy.

## Milestones and bounded exit conditions

### M001 — Generic job completion contract and background test vertical slice
Status: closed at `e828bebf` (closure `plans/closure/async-background-jobs/001-status.md`).
Produce durable `JobId` notification/replay identity and one fully usable model-facing `test.execution_mode=background` path. One submitted test may finish before or after its parent checks for work; model-visible completion is one event, not repeated `test` or `status` calls. No generic idle auto-resume.

### M002 — Background Bash and routed build/lint/format
Status: closed at `19028149` (closure `plans/closure/async-background-jobs/002-status.md`).
Support an explicit background option for long-running shell/build/test/lint/format commands via the canonical scheduler, without raw-shell bypass or loss of existing foreground output/audit. No implicit `auto` mode in this milestone.

### M003 — Safe agent continuation, attention delivery, and frontend controls
Status: ready (unblocked by M001 closure `e828bebf`; M002 soft only). Plan: `plans/implementation/async-background-jobs/003-safe-continuation-and-job-visibility.md`.
Durable notification displayed for ended turns; opt-in, bounded, authorized wake/continuation only where the owning task has asked to resume. Active-turn safe-boundary wake, restart, cancel and TUI state are deterministic. Never silently reopen a completed interaction.

### M004 — GitHub Actions remote CI terminal watcher
Status: blocked on M003 closure. Plan: `plans/implementation/async-background-jobs/004-github-actions-completion-watch.md`.
Watch one authorized, immutable GitHub run/commit/check identity, poll with daemon-owned finite scheduled observations (not LLM or shell), and emit one final notification when the remote CI reaches a terminal status. Safe cancellation/restart/rate limits and unverified/ambiguous results are explicit.

## Cross-cutting verification and closure

Use deterministic fake executors and local HTTP fixtures. Exercise multi-session authorization, two simultaneous frontends, crash/transaction failpoints, already-completed-on-subscribe, event replay, process-tree cancellation, never-resubmit non-idempotent work, and notification/channel overflow. Reuse `tests/scheduler_*`, `tests/tool_program_m016_notification_replay.rs`, `tests/tool_program_m017_notification_confirmation.rs`, `tests/tool_program_background.rs`, `agent::run_control` and existing CLI/TUI suites. Baseline commands: `cargo fmt --all -- --check`, focused `cargo test` suites, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `scripts/verify.sh quick`, `python3 scripts/check_scheduler_bypass.py`, `python3 scripts/check_execution_ownership.py`, `bash scripts/check-core-boundary.sh`; host CI only via existing workflows if closure evidence requires it.

Per-milestone `plans/closure/async-background-jobs/00N-status.md` must record exact head/commit, behavior/evidence matrix, known residuals, security, cross-platform limitations, verification commands/results and precise downstream unblocking. No plan should be marked closed solely on the presence of a job handle.

## Deferred, not silently implied

Multi-provider external CI platforms, arbitrary URL watches/webhooks, general cron/BPM/DAG engine, unconsented overnight self-driving agent turns, every direct/read-only tool as a job, cross-daemon distributed watches, indefinite waits, sandbox expansion, generic provider callback infrastructure, and autonomous model decisions on CI results are out of scope. An ADR is required only if implementation discovers a genuine unresolved cross-domain authority decision; otherwise consume existing ADR-0001/0002.
