# Async Background Jobs M003 — Safe Continuation, Notification Inbox and Frontend Visibility

Status: blocked — M001 must close
Baseline: `e6a5e85de632ea5057d59aee585942d10f9d950b` (revalidate before implementation)
Roadmap: `plans/subsystems/async-background-jobs-and-ci-watch-roadmap.md#m003--safe-agent-continuation-attention-delivery-and-frontend-controls`
Class: capability + invariant. Hard dependency: M001. Soft dependency: M002 for Bash user experience; work may proceed against M001 test jobs.

## Objective

Complete the delivery lifecycle when an asynchronous job finishes *after* its originating `AgentLoop::run()` has returned. Provide useful default **notify-only** behavior and an explicitly authorized, bounded `resume_owner` policy for workflows such as “keep working after this test passes.” A live parent may receive the result at the next safe boundary; a dormant parent must not lose it or silently consume new inference.

Do not implement infinite autonomous agent loops, a new chat runtime, external messaging, general workflow engine, user impersonation, or implicit background model wakeups for unrelated jobs.

## Existing seams and observed defect

`src/agent/follow_up.rs::drain_follow_up` uses `try_recv()` and explicitly states that messages arriving after `run()` returns are not handled by that invocation. `src/agent/loop.rs::follow_up_sender` confirms the same. `src/agent/turn_runtime.rs` registers/unregisters exact live `(session_id, turn_id)` senders. `src/agent/run_control.rs` routes live child/group completions but logs dropped sends when a channel is closed or full; durable run/job records remain authoritative. Existing Tool Program recovery creates confirmed session events at the next invocation and MUST retain its M017 guarantees.

## Policy and authority decisions

- Three typed completion dispositions: `notify_only` (default; persist and surface), `resume_owner` (agent continuation only after explicit bounded task-level authorization), `suppress` (e.g. cancelled/archived owner, with audit/inspect). User-initiated cancellation or a fresh steer/replacement turn may invalidate an outstanding resume lease. A model cannot create a more privileged resume authorization than the session's originating user/task contract.
- A resume lease must bind immutable session/owner/principal/workspace/turn ancestry, accepted job/notification ID, authorized continuation mode, expiry, maximum wakes and model/tool/inference budget. Freeze provider/model policy at the authorized boundary or resolve a clearly documented equivalent without silently widening privileges. No saved raw credential/tokens.
- **Exactly one accepted model-continuation request per logical terminal notification**, with durable claim/started/finished or recoverable state. Loss of in-memory sender is not loss of authority; event append, ack and model-turn start cannot be treated as one atomic step unless durable recovery reconciles each boundary.
- Job completion can be delivered to active root turn only via the exact owning turn or direct parent run; no session-wide opportunistic injection into a different live turn and no duplicate provider call while a provider call is already in flight. An ended turn does not retain a stale sender.
- For `notify_only`, persist a redacted attention/session event and let frontends show it; next model turn can inspect and consume it without duplicate insertion. For `resume_owner`, queue a normal canonical authorized turn through the existing turn-runtime/JobSubmissionService path; do not recursively call `AgentLoop::run()` from a scheduler callback or create a competing inference queue.
- Deadline/lease expiration, denied role, revoked project access, unavailable provider, archived session, full mailbox, daemon restart, or exhausted budget result in explicit `needs_attention`/suppression—not hidden retry loops or user charges.
- External actor's job status data is untrusted; system-generated completion message is authenticated by job-store terminal state and host-provided ownership. No arbitrary remote text may enter a system-role message as trusted instruction.

## Ordered work packages

A. **Delivery state diagram and failing tests.** Define `Pending -> SessionEventCommitted -> AvailableToOwner/ResumeQueued -> Consumed/Expired/Suppressed` (exact state names flexible) and guard CAS transitions. Tests must show current ended-turn drop and distinguish live active loop versus ended loop. Include dual frontends and duplicate producer/replay.

B. **Resume authorization and queue.** Add the smallest policy/record in the canonical session/turn notification domain. Use existing scheduler/turn runtime to allocate a new authorized continuation turn, with atomic idempotency key derived from stable notice+lease; avoid any new scheduler or direct model call in notification handler. Refuse on unauthorized/expired/revoked/archived owner. Make default conservative and allow a user/operator to opt into continuation for one background wait.

C. **Live safe-boundary routing.** Wake only the exact registered owner. Coordinate notifier, follow-up channel, current provider step, cancel/steer and turn completion so a completion at the `run()` return boundary cannot be lost or processed by the wrong turn. Do not synthesize a new user utterance or blindly reopen a completed turn. Durable event remains available and status distinguishable even if a live hint cannot be delivered.

D. **TUI/remote client projection.** Expose bounded active/running/completed/failed/cancelled background job list and pending completion notices over existing project/session projection protocol. Present handle, job kind, compact status, provenance-aware artifact link, active continuation intent and explicit cancel/wait/inspect. Respect observer/redaction/role capabilities, reconnect snapshot/replay, out-of-order events, multi-client scoped visibility, TUI shutdown. Do not spawn TUI polling tasks as the source of truth.

E. **Agent affordances.** Keep foreground default, tell model that notifications arrive automatically, show when `resume_owner` was not authorized and that `job.wait` is a bounded manual observation. No “please poll every 10 seconds” scripts in system prompt or self-created cron jobs.

F. **Evidence/closure.** Document model-continuation guarantees, budget and privilege boundary, user-facing resume controls, audit event identity, recovery and stop semantics in `architecture/agent.md`, `architecture/jobs.md`, `architecture/projection.md`, daemon/TUI docs as affected. Register closure and downstream M004 unblock only after positive tests.

## Acceptance matrix

- Live parent: completion during unrelated work produces one safe-boundary notice without duplicate provider entry.
- Ended turn with default policy: no model tokens spent; one durable visible notification, inspectable on reconnect/restart and next turn.
- Explicit authorized resume: one new normal turn starts on terminal completion, *never* another under duplicate terminal events or simultaneous frontends, and within bound/lease. User cancel, session archive, policy revocation or expiry wins before model invocation.
- Crash at each boundary (notice persistence, event append, ack, resume claim, new turn record, provider start) leaves reconstructable state without duplicate unbounded inference. Provider-start ambiguity must be recorded as attention unless idempotent replay is actually proven.
- Failure/timeout/cancellation/Interrupted-and-retry each classified correctly; old/different session receivers receive none; observer principal cannot read secret content or cancel owner's job.
- TUI and daemon protocol show equal authoritative state on reconnect and recovery; no success/toast-only completion masquerades as durable receipt.

Run focused `agent::follow_up`, `agent::run_control`, `tests/tool_program_m016_notification_replay.rs`, `tests/tool_program_m017_notification_confirmation.rs`, `tests/scheduler_restart_recovery.rs`, session projection consumer/security suites, M001 job-notification tests, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `scripts/verify.sh quick`, applicable core/projection/authorization guards. No live-provider or permanent new CI workflow.

Stop on non-idempotent inference restart ambiguity: record `needs_attention`, do not replay blindly. Closure: `plans/closure/async-background-jobs/003-status.md`.
