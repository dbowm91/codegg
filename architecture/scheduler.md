# Scheduler-owned execution

Codegg's daemon is a scheduler-owned execution service. A daemon operation
that starts a process, runs a test/build/lint/format command, dispatches a
subagent, or consumes a constrained machine resource must enter the durable
job store before it runs.

The production path is:

~~~text
frontend/tool/TUI request
  -> CoreDaemon / JobSubmissionService
  -> durable JobRecord
  -> JobScheduler fair queue
  -> JobAttempt + ResourcePermitGuard
  -> typed JobExecutor
  -> canonical domain service
  -> durable terminal attempt/job state + bounded completion
~~~

--standalone and the hidden stdio compatibility mode are explicit non-daemon
harnesses. They may retain local compatibility services, but they do not
provide the machine-wide singleton or global admission guarantees.

## Submission boundary

src/scheduler/submission.rs contains JobSubmissionService, the daemon-owned
facade for creating work. It validates the workspace service lease and
job-kind payload, rejects oversized payloads, applies
ResourceRequest::for_kind, normalizes namespaced exclusivity keys, creates one
durable JobRecord, wakes the scheduler, and optionally coalesces retries using
an opaque SubmissionKey.

Submission-key idempotency is currently in-memory for one daemon generation.
The durable job ID remains authoritative after a response is lost; a future
storage migration can persist the key/fingerprint when cross-restart retry
identity is required.

The protocol supports JobSubmit, JobWait, JobGet, JobList, JobAttempts, and
JobCancel. Clients never provide attempt IDs, daemon generation, permits, or
executor implementation details.

## Scheduler and admission

JobScheduler owns reconciliation, fair queue selection, admission, executor
lookup, attempt lifecycle, cancellation signalling, and completion
persistence. ExecutorRegistry is keyed by ExecutorKind.

| Job kind | Executor | Canonical service |
|---|---|---|
| Test | TestJobExecutor | test_runner::resolve_and_run_test |
| Build, Lint, Format, ManagedProcess, Shell | ManagedArgvExecutor | ManagedProcessService |
| Subagent | SubagentJobExecutor | SubAgentPool::send_and_wait |

Every executor context contains a typed AttemptId, the active daemon
generation, a workspace lease, a cancellation token, and a live
ResourcePermitGuard. Runtime validation rejects an empty identity or a
controller-less permit before executor code runs. The scheduler records the
executor name on the attempt before marking it running.

The scheduler default is enabled and mandatory. Explicit enabled = false
creates an introspection placeholder whose submission API returns
SchedulerDisabled; daemon tools do not fall back to direct process creation.
observe and active remain accepted configuration labels for staged deployments
and diagnostics, but they do not restore bypass execution.

Admission reserves soft CPU/memory/IO hints, process slots, network slots, and
typed exclusivity keys. Hints are accounting inputs, not OS-enforced resource
limits. Conservative defaults are centralized in
codegg_core::jobs::ResourceRequest::for_kind:

| Kind | CPU | Memory hint | Processes | IO | Network | Default conflict |
|---|---:|---:|---:|---:|---:|---|
| Test | 2 | 1024 MB | 1 | 2 | 0 | — |
| Build | 3 | 2048 MB | 1 | 3 | 0 | exclusive:workspace-mutation |
| Lint | 1 | 768 MB | 1 | 1 | 0 | — |
| Format | 1 | 256 MB | 1 | 1 | 0 | exclusive:workspace-mutation |
| Subagent | 1 | 512 MB | 1 | 1 | 1 | — |
| Git mutation | 1 | 256 MB | 1 | 1 | 0 | exclusive:worktree-mutation |

Impossible requests fail before executor invocation. Temporarily blocked
requests are requeued, and the bounded candidate window prevents one blocked
workspace from stopping unrelated work.

## Canonical process policy

src/managed_process.rs is the shared noninteractive argv service. It owns
sanitized inherited environment and noninteractive defaults, job/attempt
provenance variables, process session creation, descendant cleanup,
cancellation and timeout termination, drained head/tail-bounded stdout/stderr,
and typed exit/termination classification.

ManagedArgvExecutor is only an adapter. It does not call
tokio::process::Command and never falls back to a shell after admission or
spawn failure. The explicit shell route is represented as a JobKind::Shell
payload and still uses the scheduler plus the managed process service.

TestRunner remains the domain authority for framework discovery, stall
timeouts, reports, artifacts, and RunStore persistence. It is invoked only by
TestJobExecutor. TestTool, Bash test translation, and the TUI /test command
submit durable test jobs. TUI/server clients use WorkspaceRegister, JobSubmit,
and JobWait rather than constructing TestRunner locally.

## Execution-surface inventory

| Production caller | Target kind | Executor/service | Status |
|---|---|---|---|
| src/tool/test.rs | Test | TestRunner | Scheduler submission |
| src/tool/bash.rs test translation | Test | TestRunner | Scheduler submission |
| src/tool/bash.rs build/lint/format/managed routes | matching kind | ManagedProcessService | Scheduler submission |
| src/tool/bash.rs explicit shell route | Shell | ManagedProcessService with sh -c payload | Scheduler submission |
| src/tui/commands/test.rs | Test | daemon protocol + TestRunner | Scheduler submission |
| server CoreRequest::JobSubmit | typed caller kind | daemon submission facade | Scheduler submission |
| scheduler subagent adapter | Subagent | SubAgentPool | Scheduler admission; waits for worker result |
| src/job_dispatcher.rs | Subagent | SubAgentPool | Definition retained; no daemon production wiring |
| legacy BackgroundScheduler | Subagent | local pool | Standalone compatibility only |
| typed Git services / native Git read fallback | GitRead/mutation | egggit/Git service | Domain-specific compatibility path; migration remains |
| interactive terminal/editor/formatter helpers | explicit user/local action | local process API | Not daemon heavy-job submission yet |

The last three rows are deliberately documented rather than hidden behind
the static guard: they are compatibility or domain-specific surfaces whose
full scheduler submission requires additional RunStore/PTY integration.
They must not be described as covered by the daemon invariant until migrated.

## Lifecycle and recovery

Scheduler dispatch creates an attempt, persists executor provenance, marks it
running, registers cancellation before spawn, and persists exactly one
terminal completion. Completion records are bounded in memory for waiters;
full artifacts remain in RunStore.

Cancellation removes queued entries and signals matching running attempts.
Managed-process cancellation kills the process session and descendants before
the permit is released. A completion that races cancellation follows the
durable store's terminal-state precedence.

At startup, recover_generation marks stale attempts interrupted and applies
the persisted idempotency/retry policy. Queue reconciliation rebuilds the
in-memory fair queue from durable queued jobs. Schedule occurrence uniqueness
is enforced by (schedule_id, scheduled_for); legacy background tasks are
migrated to ScheduleStore, while standalone compatibility task loops remain
explicitly outside daemon guarantees.

## Operator visibility

SchedulerSnapshot is bounded and includes queued/running counts,
per-workspace counts, configured resource budgets, current usage, executor
health, admission-block counters, queue overflow counters, and oldest queued
age. SchedulerEvent carries bounded deltas and IDs; clients fetch full job and
attempt records through protocol requests. JobWait returns a bounded
completion summary and optional RunStore ID.

## Static and runtime proof

scripts/check_scheduler_bypass.py rejects direct TestRunner calls outside
scheduler executors and test fixtures, and rejects production use of the old
dispatch_to_test_runner name. The legacy subagent dispatcher is retained only
as a definition for compatibility and has no construction site in daemon
wiring.

Focused validation:

~~~bash
rtk cargo test -p codegg --lib scheduler
rtk cargo test -p codegg --lib managed_process
rtk cargo test --test scheduler_phase5
rtk cargo test --test durable_jobs_phase4
rtk python3 scripts/check_scheduler_bypass.py
rtk python3 scripts/check_daemon_cwd_usage.py
~~~

The current proof covers resource admission, one-process-cap contention across
two temporary workspaces, fairness/exclusivity primitives, submission
idempotency, disabled-scheduler behavior, managed-process timeout, bounded
output, cancellation cleanup, and durable recovery. A future operational
extension should add shared Cargo-target aliasing and process-tree fixtures.
