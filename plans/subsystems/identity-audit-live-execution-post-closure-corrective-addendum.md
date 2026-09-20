# Identity / Audit — Live Execution Post-Closure Corrective Addendum

Status: active

Repository baseline reviewed: `a996e20060a0103a152a80fb62463241c1fd1162`

Predecessor work:

- `plans/subsystems/identity-authorization-audit-roadmap.md`
- `plans/closure/identity-authorization-audit/005-status.md`
- `plans/closure/team-collaboration-post-closure-corrective/003-status.md`
- `architecture/audit.md`

Long-term requirements:

- `plans/000-long-term-specification.md#22-audit-architecture`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/003-planning-process.md#7-corrective-passes`

No new ADR is required to complete the already selected append-only audit architecture. If implementation would move audit authority away from the daemon/coordinator, add a public protocol, or create a second audit store, stop and register an ADR.

## 1. Corrective trigger

Identity/audit M005 intentionally closed with three low-severity live-instrumentation gaps:

1. `command_execute` exists as a typed builder/fixture but has no canonical live ToolBroker/process execution hook;
2. `git_operation` exists as a typed builder/fixture but has no canonical live Git mutation/network/recovery hook;
3. `job_complete` has a live representative `job_retry` mapping but async scheduler success/failure/interrupted terminal transitions do not emit the terminal event.

The M003 verification reconciliation also made interactive-process create/input/terminate operations explicitly uninstrumented. For command execution, the long-term specification is explicit: command execution and Git operations are auditable structural events. These are now the remaining single-host execution-audit closure items; distributed `node_enrollment`/`remote_execute` remain future distributed-execution scope.

## 2. Current architecture constraints

- `CoreDaemon::append_audit_event` is the current bounded append owner and is used by daemon family handlers.
- Typed builders live in `codegg-core::audit_instrumentation` and require a trusted `AuthenticatedPrincipal` plus `AuditDecisionProvenance`.
- `ToolBroker` is the canonical production tool-call boundary, but `BrokerInvocationContext` currently carries `principal_ref` rather than the complete trusted principal/provenance object and reconstructs `ToolExecutionContext` with `origin_principal/origin_auth_method/origin_decision_id == None`.
- `ToolExecutionContext` already defines trusted origin fields populated by daemon/permission boundaries on some paths; these must not be fabricated from model or payload strings.
- `GitMutationExecutor` is the canonical local mutation owner shared by native Git and routed Bash->Git paths; network and recovery operations use the same executor family.
- Interactive-process requests are served by the daemon pre-router with transport-bound client authority already available.
- Durable jobs persist source attribution and terminal attempt state; scheduler terminal transitions, not retry requests, are the correct owner for `job_complete`.

## 3. Invariants

- Audit actor/provenance always derives from a trusted transport/daemon decision, never a caller/model supplied string.
- The coordinator remains the sequence/store authority; execution owners may emit only through one injected bounded audit-emission seam.
- No command text, argv, terminal input, Git URL credentials, file bodies, or tool output enter audit metadata; use structural digests and bounded labels.
- One real transition emits at most one structural event; retries/idempotent replays do not duplicate events.
- Audit failure remains bounded/best-effort per the accepted M005 policy and surfaces counters/warnings.
- ToolBroker, scheduler, and Git execution ownership are not bypassed to obtain audit coverage.
- Personal-local and team execution use the same internal audit composition.

## 4. Non-goals

- Distributed node/remote-execution audit.
- Audit body retention redesign, hash chaining, signing, or a new store.
- Logging ordinary chat bodies, terminal keystrokes, command text, or secrets.
- Replacing ToolBroker, JobScheduler, GitMutationExecutor, or interactive-process architecture.
- Adding audit to read-only Git facts unless needed for a mutation chain.
- Turning every high-volume status/list/read into an audit event.

## 5. Dependency graph

```text
M001 trusted execution audit context/emitter
      ├──────────────> M002 command + interactive + Git live hooks
      └──────────────> M003 scheduler job-complete live hook
M002 + M003 ─────────> M004 qualification and coverage-guard closure
```

M001 is ready. M002/M003 are hard-blocked on M001. M004 is hard-blocked on M002+M003.

## 6. Milestones

### M001 — Trusted execution audit context and emitter seam

Establish one cloneable, bounded, daemon-owned/injected emission service plus a trusted execution audit context carrying the actual principal/provenance/chain data needed by non-daemon execution owners. Thread it through ToolBroker/scheduler/Git composition without emitting new events yet except focused seam tests.

Plan: `plans/implementation/identity-audit-live-execution-post-closure-corrective/001-trusted-execution-audit-context-and-emitter.md`.

### M002 — Command, interactive-process, and Git live audit hooks

Use the M001 seam to emit `command_execute` at canonical process/tool execution ownership and `git_operation` at the canonical Git mutation/network/recovery owner. Interactive-process creation emits structural command execution without logging terminal input bodies.

Plan: `plans/implementation/identity-audit-live-execution-post-closure-corrective/002-command-interactive-git-live-audit-hooks.md`.

### M003 — Scheduler terminal job-completion audit

Emit `job_complete` at the durable terminal attempt transition for success/failure/interrupted outcomes using persisted source attribution and deterministic correlation. Do not use `job_retry` as a proxy once the true terminal hook exists.

Plan: `plans/implementation/identity-audit-live-execution-post-closure-corrective/003-scheduler-job-complete-live-audit.md`.

### M004 — Live execution audit qualification and guard closure

Tighten executable coverage so the three live hooks cannot silently regress to builder-only status, run end-to-end attribution/secret/idempotency trajectories, update docs, and close the addendum.

Plan: `plans/implementation/identity-audit-live-execution-post-closure-corrective/004-live-execution-audit-qualification.md`.

## 7. Completion definition

This corrective closes when, on a single-host deployment:

- an authorized model/tool command produces one queryable `command_execute` structural event linked to the trusted principal, decision, project/session/turn/run/job where available;
- interactive process creation produces structural command-execution audit without argv/input disclosure;
- typed Git mutation and network/recovery mutations produce one `git_operation` event with operation label plus ref/locator digest only;
- durable jobs emit `job_complete` on real terminal success/failure/interrupted transitions;
- retries/restarts do not duplicate terminal events;
- secret-negative and unauthorized-principal tests remain green;
- audit coverage guard distinguishes live executor hooks from builder-only declarations; and
- all known single-host M005 low live-hook findings are closed.

Distributed `node_enrollment` and `remote_execute` remain outside this workstream until those capabilities exist.

## 8. Milestone status

| Milestone | Status | Dependencies |
|---|---|---|
| M001 | closed (`plans/closure/identity-audit-live-execution-post-closure-corrective/001-status.md`; implementation `7c75c009`) | identity M005 closed; M003 verification guard closed |
| M002 | closed (`plans/closure/identity-audit-live-execution-post-closure-corrective/002-status.md`; implementation `e0cc7f05`) | hard dependency on M001 satisfied (M001 closed) |
| M003 | ready | hard dependency on M001 satisfied (M001 closed) |
| M004 | blocked | hard dependency on M002 + M003 |
