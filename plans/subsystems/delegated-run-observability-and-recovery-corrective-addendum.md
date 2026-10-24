# Delegated Run Observability and Recovery — Corrective Addendum

Status: closed — all four milestones closed on 2026-10-24; ADR-0051 accepted (option B) on 2026-10-24

Planning baseline: `a21b5fffa` on `fix/tui-composer-helix-parity-post-closure-corrective` (2026-10-24). Revalidate the actual implementation head before any code change.

Long-term references:

- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#28-observability`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md` — agent run, delegated run, session
- `plans/002-long-term-roadmap.md` (durable agent hierarchy and recovery)
- `plans/003-planning-process.md` §§6–7, 10–11

Related ADRs:

- `plans/adrs/ADR-0031-authenticated-idle-continuation-delegated-turn-admission.md` — accepted; the durable notification and continuation machinery this line reuses.
- `plans/adrs/ADR-0041-subagent-run-addressing-and-human-interaction-authority.md` — accepted; the human steers the parent, not the child.
- `plans/adrs/ADR-0043-session-owned-subagent-lifecycle-projection.md` — accepted; the delegated lifecycle projection this line extends.
- `plans/adrs/ADR-0051-durable-delegated-run-transcript-and-continuation-authority.md` — **accepted** (option B) on 2026-10-24; gates C002–C004.

Predecessors (history retained; not reopened): `plans/subsystems/tui-subagent-and-job-visibility-roadmap.md`; `plans/subsystems/tui-subagent-and-job-visibility-convergence-corrective-addendum.md`; `plans/subsystems/tui-live-work-visibility-corrective-roadmap.md`; `plans/subsystems/async-background-jobs-and-ci-watch-roadmap.md`; `plans/subsystems/async-background-jobs-idle-completion-corrective-addendum.md`; `plans/subsystems/agent-loop-operational-reliability-roadmap.md`. Their closure records stay closed and unaltered; this addendum opens a **new** workstream rather than reopening any of them.

## 1. Corrective purpose

A delegated run's conversation is not durable, so the orchestrator's only view of delegated work is a bounded summary string pushed in-process, and the user's only view is a status line. The line was opened from a reported symptom: *"we can not seem to directly see a subagent's buffer … i've seen previous runs seemingly struggling to get any info on a subagent besides that it had ran."*

The symptom has two independent causes, and only one of them is a TUI surface question:

- The **daemon never creates the transcript**. The child's messages are built in memory per attempt and the durable write is a deliberate no-op, because the synthetic session id has no `session` row and `message.session_id` is a foreign key with `ON DELETE CASCADE`. A transcript surface would have nothing to render. This is a data-model gap, not a rendering gap.
- The **orchestrator's terminal notification is best-effort and lossy twice** — a `try_send` that is dropped when the parent's channel is full, and a lookup that misses entirely when the parent's turn has already ended. The delegated job is not a subscriber of the durable notification machinery that `bash` and `test` already use.

The purpose of this addendum is to close both, and then to make a failed delegated run continuable instead of disposable.

## 2. Discovered corrective findings

### F1 — A delegated run's transcript is never persisted

`src/agent/worker.rs:1765-1773` builds the child's messages per attempt; `src/agent/worker.rs:1545-1547` names the child's synthetic session `{parent_id}-sub-{task_id}`; `crates/codegg-core/src/session/store.rs:2321-2324` returns `Ok(None)` from `append_batch` because that session has no row; `src/agent/turn_completion.rs:567-568` records the position as "Synthetic transcripts (subagents, continuation tasks) have no durable session row; their output stays in the run journal." The projection then carries none of it: `crates/codegg-protocol/src/projection/dto.rs:545-548` and `crates/codegg-core/src/projection_replay/agent_runs.rs:92`.

**Unclosed requirement:** a delegated run's conversation is a durable, bounded, redacted, addressable artifact. **Owner:** ADR-0051, then C002.

### F2 — The orchestrator's notification of a child's terminal state can be lost twice

`src/agent/run_control.rs:406-443` pushes `"Child run {run_id} finished with status {status}. Summary: {summary}"` (bounded to 2 KiB, `src/agent/run_control.rs:16`). The push is a `try_send` on a bounded channel: `src/agent/run_control.rs:414-420` drops it when the channel is full or closed, with only a `tracing::warn!`. Separately, `src/agent/turn_runtime.rs:1124` unregisters the parent's live turn when the turn ends, so a child that finishes afterwards matches no live target and its terminal state reaches nobody. The legacy, non-durable path has no push at all (`src/agent/worker.rs:814-860`).

**Unclosed requirement:** the orchestrator learns the terminal state of every delegated run it spawned. **Owner:** C001.

### F3 — The delegated job does not participate in the durable notification machinery

`JobNotificationService`, `JobCompletionDisposition` (`src/scheduler/job_notifications.rs:197-202`), the single-use `JobContinuationLease` and the crash-safe recovery loop in `src/agent/job_notification_recovery.rs` are accepted under ADR-0031 and shipped for background jobs. The subscribers are `src/tool/bash/process.rs:1614` and `src/tool/test.rs:447`. The `task` tool never calls `subscribe_with_options`.

**Unclosed requirement:** a delegated run's terminal state is a durable notice row, not an in-process message. **Owner:** C001.

### F4 — The orchestrator cannot read a delegated run's actual output on the durable path

`src/scheduler/executors.rs:1464` stores `result_ref = agent-run-result:{run_id}`; `src/tool/task.rs:1819-1831` returns `Result: <that pointer>` to the model. No `task` action dereferences it, and the action enum at `src/tool/task.rs:1278` has no reader for it. The orchestrator therefore receives a pointer where it needs content.

**Unclosed requirement:** the orchestrator can obtain the child's recorded result. **Owner:** C001.

### F5 — A failed delegated run cannot be continued

`src/tool/task.rs:1278` has no `resume` action; `src/tool/task.rs:2096-2097` submits the delegated job with `RetryPolicy::no_retry()` and `IdempotencyClass::NonIdempotent`, so `retry_job` is refused and daemon-restart requeue refuses it; `src/scheduler/executors.rs:1724-1729` sets `Retryability::RequiresRecovery` only for a git-conflicted repository. The child's `ChatRequest` is rebuilt from scratch on every attempt, so nothing carries a prior attempt's context forward.

**Unclosed requirement:** a failed delegated run is continuable by the principal that spawned it, under the parent's authority. **Owner:** ADR-0051, then C004.

## 3. Work classification

### Invariants

- A delegated run's authority is its parent's. Nothing in this line widens a delegated run's tools, paths, sandbox or worktree ceiling, and nothing gives it a session or a turn of its own.
- A delegated run is **not** a user session. It never acquires a `session` row and never enters session enumeration, project binding, `/sessions`, `Ctrl+T`, or any session-scoped snapshot.
- The durable notification store is the only notice store. No second notice mechanism is introduced for delegated runs.
- The scheduler's admission authority, the `ToolBroker`-only dispatch boundary and the delegated worktree isolation are unchanged.

### Capabilities

- **C001** — the orchestrator learns the terminal state of every delegated run it spawned, durably, and can read that run's recorded result.
- **C003** — the TUI renders a delegated run's conversation as a transcript buffer.
- **C004** — a failed delegated run can be continued by the principal that spawned it.

### Infrastructure

- **C002** — the durable delegated-transcript store, its redaction at write, and its bounds.

### Polish

- The `task` tool's own copy, the `docs/tui.md` and `architecture/` statements about what a delegated run shows, and the completion notice's wording. These follow the capability they describe and are not a separate milestone.

## 4. Non-goals

- **Not** a second scheduler, a second notice store, a delegated-run retry policy the scheduler does not already own, or any provider call from a notification callback.
- **Not** a change to ADR-0041: the human still steers the parent, not the child. C003 makes a child's transcript *visible*; it does not give the human a channel to the child.
- **Not** a change to ADR-0031: C001 subscribes delegated runs to the existing machinery with the disposition fixed to `NotifyOnly`. `ResumeOwner` for delegated runs is **not** authorized here; continuation is C004's subject and requires ADR-0051.
- **Not** the retirement of the legacy, non-durable delegated path, and not any change to it.
- **Not** parent-session compaction applied to a child transcript, and not any session-level summarization of delegated work.
- **Not** a project-wide transcript viewer, an audit log, or a retention product.
- **Not** a change to delegated run isolation, the writer fence, the worktree ceiling, the sandbox ceiling, or the shared-checkout writer contract.

## 5. Current state

- A delegated run's conversation: in-memory only; no durable store; no projection; the TUI renders a run inspector (`src/tui/app/render.rs:1563-1697`).
- The orchestrator's terminal knowledge: a ≤2 KiB in-band notice, best-effort (`src/agent/run_control.rs:406-443`), drained once per turn (`src/agent/loop.rs:2176`).
- The orchestrator's durable-path result: an unreadable pointer (`src/scheduler/executors.rs:1464`, `src/tool/task.rs:1819-1831`).
- The delegated job's notification: no durable row, no continuation lease, no idle recovery.
- Continuation: none. The `task` action set has no `resume`; the delegated job is `no_retry`/`NonIdempotent`.
- The `task` tool's own copy already promises more than the delivery guarantees: "prefer wait or push notifications over polling get/status" and "Child completion never merges into the parent automatically" (`src/tool/task.rs:1268`).
- `src/agent/worker.rs:1806-1818` records the child's output as the concatenation of its `TextDelta` events, and treats an empty response as success ("completed with {n} events (no text output)").

## 6. Target architecture

```
                  ┌──────────────────────── daemon ────────────────────────┐
  spawn ─────────▶│ delegated job (scheduler, unchanged admission)         │
                  │   └─ executor ─▶ child agent loop                      │
                  │        ├─ messages ─▶ transcript store (C002)  ◀── ADR-0051
                  │        └─ terminal ─▶ JobNotificationService (C001)      │
                  │                        └─ notice row (NotifyOnly)        │
                  │                              └─▶ next parent turn sees it│
                  │        └─ failure ─▶ continuation (C004) ◀── ADR-0051    │
                  └────────────────────────────────────────────────────────┘
                                            │
                  projection seam (C003)    │  bounded, redacted transcript DTO
                                            ▼
                                    TUI transcript buffer
```

The load-bearing properties: one transcript store, one notice store, redaction at write, bounds at write, and continuation that reads back the recorded transcript before any provider entry.

## 7. Milestones

### C001 — The orchestrator always hears about its child

Class: **capability**. Depends on: nothing (ADR-0031's machinery is accepted and shipped). Plan: `plans/implementation/delegated-run-observability-and-recovery-corrective/001-the-orchestrator-always-hears-about-its-child.md`.

Objective: the delegated job becomes a subscriber of the durable notification service with the disposition fixed to `NotifyOnly`, so every delegated run's terminal state is a durable notice row rather than an in-process push that can be dropped; and the `task` tool can hand the orchestrator the child's recorded result instead of an opaque pointer.

Exit conditions: the durable notice exists for success, failure and cancellation; a parent turn that ends before its child still finds the notice on its next turn; the dropped-`try_send` path is either removed or proven unreachable; `task action=get` returns the child's recorded result within a declared bound.

### C002 — A durable transcript for a delegated run

Class: **infrastructure**. Depends on: **ADR-0051 accepted**. Plan: `plans/implementation/delegated-run-observability-and-recovery-corrective/002-a-durable-transcript-for-a-delegated-run.md`.

Objective: the child's conversation is written, redacted at write, to a dedicated store keyed by the run id, with per-row and per-run bounds and a recorded truncation marker.

Exit conditions: a delegated run's messages are durably present and redacted; a known-secret-shaped body is provably absent from the stored row; the per-run bound is enforced and truncation is recorded; no `session` row is created and the session catalog is unchanged.

### C003 — The TUI renders the delegated transcript

Class: **capability**. Depends on: **C002 closed**, **ADR-0051 accepted**. Plan: `plans/implementation/delegated-run-observability-and-recovery-corrective/003-the-tui-renders-the-delegated-transcript.md`.

Objective: the delegated run's tab becomes a real transcript buffer rather than a run inspector, with the run inspector's metadata retained.

Exit conditions: the tab renders the recorded conversation; the payload is bounded and refused rather than silently truncated; the projection guards cover the new payload; a client that does not know the transcript field still works.

### C004 — A delegated run can be continued

Class: **capability**. Depends on: **C002 closed**, **ADR-0051 accepted**. Plan: `plans/implementation/delegated-run-observability-and-recovery-corrective/004-a-delegated-run-can-be-continued.md`.

Objective: a failed delegated run is continuable by the principal that spawned it — a new attempt of the same run, under the parent's authority, seeded from the recorded transcript.

Exit conditions: every denial case fails closed before any provider entry; the continuation is exactly-once across a crash at each boundary; the parent's ceiling is not widened; a run recorded before C002 refuses continuation with a typed reason.

## 8. Cross-cutting requirements

- **Storage.** Additive migrations only. No existing table is rewritten, no existing row removed, no backfill.
- **Protocol.** Additive, `#[serde(default)]`-tolerant DTOs, so an older client works against a newer daemon.
- **Security.** Redaction at write; bounds at write; continuation authorized by the spawning principal under the parent's ceiling; no credentials, environment values or raw provider reasoning in a transcript body; a transcript is not an audit log.
- **Concurrency.** The transcript store tolerates concurrent appends for one run with a stable ordinal; the notice row is written exactly once per logical terminal; a continuation is claimed by a compare-and-swap and is never replayed blind.
- **Observability.** A dropped notice, a truncated transcript and a refused continuation are each visible in the daemon's logs — never silently discarded.
- **Docs.** `architecture/agent.md`, `architecture/tui.md`, `architecture/projection.md` and `docs/tui.md` state what a delegated run shows and what the orchestrator knows, and they state it once.

## 9. Verification strategy

- Narrow first: the crate or family that owns each change, then the workspace check.
- `bash scripts/verify.sh quick` is the canonical gate and must exit 0.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` must be clean. Never `--all-features`.
- Root integration tests are families: `cargo test --test <family> -- <member>`.
- Any new static guard carries a `--self-test` and is wired into **both** `scripts/verify.sh`'s `run_guards()` and `.github/workflows/ci.yml`.
- `scripts/rust_test_scan.py --validate` is the ground truth for test scanning.
- C001's evidence must include a **negative** test that the pre-change tree fails; a passing test that the baseline also passes is not evidence.

## 10. Risks and decision points

### Risks

- **The transcript becomes an unbounded log.** Bounded per row and per run at write; truncation recorded, never silent.
- **A transcript leaks a secret.** Redaction at write, proven by a test that inspects the stored row, not the rendered view.
- **Continuation becomes a second execution authority.** It runs under the parent's ceiling, is authorized by the spawning principal, is bounded to one per terminal failure, and is claimed by a compare-and-swap.
- **The session catalog is polluted by delegated runs.** No `session` row is created; the milestone proves the catalog is unchanged.
- **A second notice store appears.** The delegated job subscribes to the existing service; no parallel mechanism.

### Decision points

- **D1 — the durable identity of a delegated transcript (ADR-0051).** **Decided 2026-10-24: option B**, accepted explicitly by the user. Options A (status quo) and C (a real `session` row reusing the message store) are rejected; C is rejected because it would change what the user's session catalog means. The decision gates C002, C003 and C004.
- **D2 — the disposition of a delegated run's terminal notice.** Fixed to `NotifyOnly` in C001. `ResumeOwner` is **not** proposed for delegated runs and is not authorized by this addendum.
- **D3 — the provider-failure retryability taxonomy.** **Decided 2026-10-24 in C004**, and recorded in `crates/codegg-core/src/agent_run_attempt.rs:57` (`DelegatedFailureClass`): a provider authentication or authorization failure is **not** continuable (the continuation presents the same credentials, so an unchanged request refused for its credentials cannot succeed); a provider rate-limit or transient upstream failure, a deadline or budget exhaustion, and a tool-permission denial **are** continuable; a cancellation is **never** continuable; and an unrecognised failure class maps to `Unclassified`, which is **not** continuable — the classification fails closed. The classes come from the repository's own `ProviderError::error_class()`, so there is no second recogniser.
- **D4 — the legacy, non-durable delegated path.** Whether it is retired, or given the same notification guarantee as the durable path, is out of scope here. C001 must state which path is live in production and must not claim a guarantee for a path it did not change.

## 11. Completion definition

This addendum is complete when C001–C004 each have an accepted closure record, or when each remaining milestone is recorded as closed-negative with its evidence, and when the registry, this addendum and the affected `architecture/` documents agree on what a delegated run shows and what the orchestrator knows. A milestone that is implemented but not closed is not complete.

## 12. Milestone status table

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| C001 | **closed** | `plans/implementation/delegated-run-observability-and-recovery-corrective/001-the-orchestrator-always-hears-about-its-child.md` | `plans/closure/delegated-run-observability-and-recovery-corrective/001-status.md` | none — closed 2026-10-24 |
| C002 | **closed** | `plans/implementation/delegated-run-observability-and-recovery-corrective/002-a-durable-transcript-for-a-delegated-run.md` | `plans/closure/delegated-run-observability-and-recovery-corrective/002-status.md` | none — closed 2026-10-24 |
| C003 | **closed** | `plans/implementation/delegated-run-observability-and-recovery-corrective/003-the-tui-renders-the-delegated-transcript.md` | `plans/closure/delegated-run-observability-and-recovery-corrective/003-status.md` | none — closed 2026-10-24 |
| C004 | **closed** | `plans/implementation/delegated-run-observability-and-recovery-corrective/004-a-delegated-run-can-be-continued.md` | `plans/closure/delegated-run-observability-and-recovery-corrective/004-status.md` | none — closed 2026-10-24 |
