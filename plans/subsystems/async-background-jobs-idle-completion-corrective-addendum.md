# Async Background Jobs — Idle Completion and Delivery Corrective Addendum

Status: active — C001 ready; C002 blocked on C001; C003 blocked on C001+C002
Reviewed implementation branch: `3b4f517938380c5aceb714c20c6f04edc5e6b39f` (`work/async-background-jobs-and-ci-watch-plans`, 2026-10-13). Handoff branch is a descendant; revalidate exact HEAD before code changes.

## Authority, scope and history

This is a **new corrective workstream** for the already-implemented `plans/subsystems/async-background-jobs-and-ci-watch-roadmap.md`. M001–M004 and their original closure records (`plans/closure/async-background-jobs/001-status.md` through `004-status.md`) remain **historically closed** and **unaltered**. This successor corrects incomplete end-to-end behavior discovered by subsequent source review; it is not an excuse to reopen the full scheduler, tool or agent-run implementations. Until C003 strictly closes, do not assert the broader "mcode-style idle completion -> notify/authorized resume" use case as fully qualified.

Normative:
- `plans/000-long-term-specification.md` (durable agent hierarchy, scheduler, recovery, invariants);
- `plans/001-terminology-and-domain-model.md`;
- `plans/003-planning-process.md` §§2.4, 4, 7–10;
- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`;
- `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`;
- `architecture/jobs.md`, `architecture/scheduler.md`, `architecture/agent.md`, `architecture/authorization.md`, `architecture/projection.md`.

The goal is to let the model submit a supervised build/test or GitHub Actions watch, return control, and **without further model polling or user turns** have the daemon observe completion and put one durable, actionable result in the owning session's attention stream. For workflows where the user/operator **explicitly authorizes** an additional model turn, optionally queue one bounded owner continuation when the result arrives. The default remains **notify_only**: zero autonomous inference spend and no silent tool execution.

## Findings and why the previous closure missed them

**F1 (medium, required): no independent remote CI observation.** `src/scheduler/ci_watch.rs` documents that observations happen only on `ci_watch status`, `AgentLoop::inject_pending_notifications` at turn start via `advance_due_watches(&session_id)`, or a test's direct `observe_once`. A sleeping/ended session never drives the watch, so a remote workflow can complete unobserved for hours and remote-expiry handling likewise never runs. The M004 record explicitly calls this a low-severity trade-off, yet its milestone claims completion watching "without polling". The test harness advanced observations manually, not as an idle daemon.

**F2 (medium, required): service lifetime, contention and partial creation.** `src/agent/turn_runtime.rs` constructs both `JobNotificationService` and `CiWatchService` **per turn**, resetting the scheduler's notification-service pointer and creating a separate `CiWatchService::in_flight` map on every turn. In-memory mutexes do not serialize concurrent observations across two turns / owners / daemon service instances. `CiWatchService::create_watch` persists `ci_watch` before calling `subscribe_with_options`; subscription errors can leave an accepted watch with no terminal-notice subscription/lease, which restart recovery must repair. The original tests primarily exercised one service instance and a new service with sequential replay.

**F3 (medium, required for opt-in behavior): 'resume_owner' is only a next-turn marker.** `src/agent/job_notification_recovery.rs` consumes the single-use continuation lease and adds text to the next invoking agent turn. `src/agent/follow_up.rs` says that late follow-ups after `run()` returns require a new run. `AgentLoop` only calls `inject_pending_notifications` at turn start. Thus `resume_owner` does **not** start any bounded continuation while idle and its current "exactly one continuation" language overstates runtime behavior. It is a valid next-turn authorized *hint*, not an autonomous continuation.

**F4 (qualification): closure validation is local/fixture-only.** M001–M004 closure records report local `verify.sh quick` and focused tests; no exact-head hosted `CI / verify`, Desktop E2E, native Windows/macOS qualification, real GitHub API contract, or real provider/daemon idle lifecycle was obtained. These are evidence gaps, not evidence of a functional defect; C003 distinguishes required deterministic release evidence from optional host/network/live-provider proofs.

## Preserved boundaries

- **One scheduler.** `JobSubmissionService` and `JobScheduler` stay the only durable machine execution authority; use existing scheduler/task admission and daemon lifecycle. A single daemon-managed due-watch trigger/coordinator is allowed, but not one timer per watch, a new jobs queue, a second independent scheduler, a detached `gh run watch`, cron-as-user-workaround, or periodic model invocations.
- An inactive CI watch costs only durable metadata; a finite due observation uses bounded network/worker admission. No 20-minute CPU/process permit or Tokio task retained per remote run.
- Crash/restart, cancellation, event duplicate, 429/403, idempotency conflict, two frontends and multiple owners must not double-consume tokens, double-poll a watch at once, or duplicate terminal notices.
- **Notify-only is complete even when no model is running:** persist one terminal result and publish an authorized session/attention update without a provider call. Persistence precedes visibility.
- **Autonomous model continuation is opt-in and separately authorized**, not inferred solely from a model-supplied JSON flag. Bind owner principal, session, workspace, original task/request and delegation authority, provider/inference/tool budgets, expiry and cancel/steer behavior. Refuse broadening permissions or starting a turn as another principal. No model invocation in the CI poll callback.
- A provider invocation started before a crash is **not** blindly replayed. Ambiguous state becomes `needs_attention`, with a bounded manual recovery path.
- Preserve foreground `bash`/`test`, M001 notification identity, Tool Program M017 confirmed-session-event contract, AgentRun M009 routing, sandbox Required fail-closed policy, permission broker, process-tree cancellation, secret redaction, GitHub allowlist/SSRF/credential protections.
- No privilege-expanding change to Windows native sandbox, distributed work, webhooks, multi-CI provider abstractions, extra permanent CI job, generic BPM engine, or global TUI state redesign.

## Dependency and execution sequence

```text
M001–M004 implementation and historical closures (available)
   |
   +-- C001 singleton daemon services + independent due-watch observations [ready]
         |
         +-- C002 opt-in idle owner turn queue / exact-once claim        [blocked C001]
                |
                +-- C003 full trajectory, security, CI/merge qualification [blocked C001+C002]
```

C001 must first make terminal observation and notify-only delivery effective without an agent turn; C002 consumes that canonical terminal event and adds only the authorized resume leg. C003 must verify both in a single daemon lifecycle. C001's design must expose a minimal event/subscription interface for C002 but must not start model turns.

| Milestone | Status | Handoff plan | Strict result |
|---|---|---|---|
| C001 idle CI watch driver, service lifetime, durable creation | ready | `plans/implementation/async-background-jobs-corrective/001-daemon-watch-admission-and-reconciliation.md` | A sleeping session's CI run is observed and terminal notice persisted; no other turn/status needed |
| C002 authorized idle resume and single-turn queue | blocked on C001 | `plans/implementation/async-background-jobs-corrective/002-explicit-idle-owner-continuation.md` | Notify-only never spends tokens; explicitly approved resume triggers at most one normal owner turn while idle |
| C003 end-to-end verification and closure disposition | blocked on C001+C002 | `plans/implementation/async-background-jobs-corrective/003-idle-lifecycle-qualification-and-closure.md` | Required deterministic real-daemon trajectory and exact-head CI; all claims/registry reconciled |

## End-state use cases and negative guarantees

1. Session S starts `ci_watch` for workflow W and ends. No user/model turn occurs. W transitions queued→in_progress→completed. One daemon-owned observation detects terminal state, one session event/attention notice is durable and visible after reconnect, and **zero** provider calls are spent for default notify-only. Status/notice is independent of foreground TUI.
2. Session S runs a long background `bash` build and ends. The scheduler completes it. One session notice is durably published while idle without requiring `job.status`.
3. An operator explicitly authorizes exactly one resume on completion within a narrow budget. Terminal notice triggers the canonical turn-runtime submission; one owner turn starts **without** an unrelated user turn. Duplicate completion, restart, another frontend, and racing cancellation never produce two turns.
4. A cancelled/archived/revoked owner or expired resume grant receives no new model invocation. A cancelled/expired remote watch never cancels the GitHub workflow and does not masquerade as a CI failure.
5. If Codegg has not been running, startup reconciliation discovers due watches and pending notices without re-running terminal work; watchdog throttling and bounded attempts prevent a restart thundering herd.

## Required verification and status governance

C001/C002: fake clock, in-process GitHub REST fixtures, SQLite-backed actual daemon lifecycle, duplicate and cross-instance claim contenders, restart between every persist/claim/dispatch boundary, 429/503/backoff, blocked owner, two sessions and two frontends. C003: a full single-unit deterministic fake-provider coding-turn trajectory (submit job + CI watch → parent exits → work completes unattended → notice persisted → optional approved single resume → terminal state), existing targeted suites, and existing hosted CI exact-head evidence.

Canonical local checks where applicable: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` (do **not** force `--all-features` against repo policy), `scripts/verify.sh quick`, scheduler ownership/core boundary/projection/authorization guards, and impacted focused test binaries. Windows/macOS, hosted GitHub and live paid provider tests must be labeled unavailable or not run when absent; **do not claim qualification that did not happen**.

Write closure records only under `plans/closure/async-background-jobs-corrective/00N-status.md`; preserve original M001–M004 closure records and historical original-roadmap status. The active registry MUST advertise this corrective as a distinct row and keep blocked successor gates until strict acceptance. An ADR is needed only if a genuinely unresolved cross-domain authority decision arises; no additional scheduler/turn framework is pre-approved by this addendum.
