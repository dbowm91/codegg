# ADR-0051: Durable Delegated-Run Transcript and Continuation Authority

Status: proposed — **not accepted; no production authority granted**

Date: 2026-10-24

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#28-observability`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md` — agent run, delegated run, session

Affected subsystem roadmaps:

- `plans/subsystems/delegated-run-observability-and-recovery-corrective-addendum.md` (new)

Related accepted decisions (unchanged by this record):

- `plans/adrs/ADR-0031-authenticated-idle-continuation-delegated-turn-admission.md` — durable background-job completion notices and the authenticated idle continuation. This ADR does **not** amend it; it names delegated runs as a distinct surface from background jobs.
- `plans/adrs/ADR-0041-subagent-run-addressing-and-human-interaction-authority.md` — the human steers the **parent**, not the child (option A). This ADR does not amend it.
- `plans/adrs/ADR-0043-session-owned-subagent-lifecycle-projection.md` — the session-owned delegated lifecycle projection (metadata only). This ADR extends that projection with a bounded, redacted transcript; it does not change the lifecycle ownership.
- `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`, `plans/adrs/ADR-0047-restricted-agent-shared-checkout-writer-contract.md` — delegated run isolation and the writer fence are unchanged.

## Context and forces

A delegated run — a subagent spawned by an orchestrator turn — currently has **no durable conversation**. The orchestrator's entire view of what its child did is a bounded summary string, and that summary reaches the orchestrator by a best-effort in-process push. The user-visible consequence is that the TUI can only render a *run inspector* (name, agent, status, task description, ids) and never a transcript, and that a failed child cannot be continued — it can only be replaced by a new one.

### The transcript does not exist

The child's conversation is constructed in memory on every attempt and is deliberately never written:

- `src/agent/worker.rs:1765-1773` — the child's messages are built per run as `[System, User(prompt)]` plus whatever the loop appends.
- `src/agent/worker.rs:1545-1547` — the child's session id is synthetic: `{parent_id}-sub-{task_id}`.
- `crates/codegg-core/src/session/store.rs:2321-2324` — `append_batch` returns `Ok(None)` when the session has no row, because "Subagent and other synthetic transcripts deliberately have no durable session, and `message.session_id` is a foreign key with `ON DELETE CASCADE`, so writing one would fail the constraint rather than create a transcript."
- `src/agent/turn_completion.rs:567-568` — "Synthetic transcripts (subagents, continuation tasks) have no durable session row; their output stays in the run journal."
- `crates/codegg-protocol/src/projection/dto.rs:545-548` — `AgentRunSummary` "contains no prompt, transcript, mailbox body, hidden reasoning, credentials, or raw logs."
- `crates/codegg-core/src/projection_replay/agent_runs.rs:92` — the projection adapter hardcodes `progress: None`.
- `src/tui/app/render.rs:1567-1572` — the tab states the position directly: "It is a run inspector, not a transcript — it shows only what the projection actually vouches for… It deliberately does not synthesize progress text."

### The orchestrator's notification is best-effort, and lossy twice

- `src/agent/run_control.rs:406-443` — on terminal state, the run's parent receives `"Child run {run_id} finished with status {status}. Summary: {summary}"`, bounded to 2 KiB (`MAX_NOTIFICATION_BYTES = 2048`, `src/agent/run_control.rs:16`).
- `src/agent/run_control.rs:414-420` — that delivery is a `try_send` on a bounded channel. If the channel is full or closed, the notice is **dropped** and only a `tracing::warn!` records it.
- `src/agent/turn_runtime.rs:1124` — a turn unregisters its live entry when it ends. If the child finishes after the orchestrator's turn has ended, the lookup misses and the notice goes **nowhere**; there is no durable fallback.
- `src/agent/loop.rs:2176` — the notice is drained once per `run()`, at the end of the turn, as a `Message::User`.

The durable path additionally hands the orchestrator an unreadable pointer: `src/scheduler/executors.rs:1464` stores `result_ref = agent-run-result:{run_id}`, and `src/tool/task.rs:1819-1831` returns `Result: <that pointer>` — an identifier no `task` action can dereference.

The legacy (non-durable) path has no push at all: `src/agent/worker.rs:814-860` updates only an in-memory `TaskStore`, so the orchestrator must poll.

### The durable notification machinery exists, and delegated runs are not in it

`JobNotificationService`, `JobCompletionDisposition` (`NotifyOnly` / `ResumeOwner` / `Suppress`, `src/scheduler/job_notifications.rs:197-202`), the single-use `JobContinuationLease`, and the crash-safe recovery loop in `src/agent/job_notification_recovery.rs` are already accepted under ADR-0031 and already shipped for background jobs. The subscribers are `src/tool/bash/process.rs:1614` and `src/tool/test.rs:447`. **The `task` tool never calls `subscribe_with_options`**, so a delegated run gets none of it: no durable notice row, no continuation lease, no idle recovery.

### A failed delegated run cannot be continued

- `src/tool/task.rs:1278` — the action enum has no `resume`; the closest neighbours are `message`, `interrupt` and `cancel`.
- `src/tool/task.rs:2096-2097` — the delegated job is submitted with `RetryPolicy::no_retry()` and `IdempotencyClass::NonIdempotent`, so `retry_job` is refused and daemon-restart requeue refuses it.
- `src/scheduler/executors.rs:1724-1729` — `Retryability::RequiresRecovery` is set **only** for a git-conflicted repository; a provider or upstream failure is classified `Retryable` but nothing consumes that for a delegated run.
- `src/agent/worker.rs:1806-1818` — the child's recorded output is the concatenation of its `TextDelta` events; an empty response is recorded as success with the text "Subagent '{name}' completed with {n} events (no text output)".

### Why the decision is required now

Three separate, already-accepted subsystems have each built the piece that this needs, and each stops exactly one step short of it: the delegated-run store owns the run but not its conversation; the notification service owns terminal notices but has no delegated subscriber; the projection owns the frontend contract but deliberately carries no transcript. Nobody owns the artifact that joins them. This ADR decides what that artifact is, who owns it, what may cross the frontend boundary, and who may continue it.

## Decision drivers

- A delegated run's conversation is a first-class artifact. It must be durable, bounded, redacted and addressable, or the orchestrator's only view of delegated work is a lossy summary and the user's only view is a status line.
- **The session catalog must keep meaning "the user's sessions."** ADR-0043 and the live-work-visibility M004 treat `/sessions` and `Ctrl+T` as the user's session list. A delegated run is not a user session and must not silently become one.
- The transcript can contain tool arguments, file contents and provider reasoning. `AgentRunSummary` carries none of it on purpose; the projection contract must state exactly what may cross, and a missing declaration must be a build failure, not a default.
- Continuation must not become a second execution authority. A delegated run's authority is its parent's; a continuation must be authorized by the same principal that spawned it, under the same ceiling, and must not mint a session, a turn, or a provider entry outside the recorded run.
- Storage growth must be bounded per run and reclaimable, and a transcript must never become an unbounded log.
- The durable notification machinery that already exists for background jobs must be reused rather than re-implemented; a second notice store would be a second truth.

## Considered options

### A. Keep the status quo — no durable delegated transcript, no continuation

The delegated conversation stays in memory; the TUI stays a run inspector; the orchestrator keeps its ≤2 KiB push; a failed child is replaced by a new one.

Benefits: zero new storage, zero new projection surface, zero new authority, no migration, and every existing closure record stays true.

Costs and failure modes: the orchestrator's view of delegated work stays lossy and non-durable, so the observable symptom the line was opened for — "previous runs seemingly struggling to get any info on a subagent besides that it had ran" — persists by construction. A parent turn that ends before its child cannot learn what happened except by polling. A failed provider turn cannot be continued, so every transient upstream fault costs a full re-dispatch with a re-stated task and a re-created workspace.

### B. A dedicated durable delegated-transcript store, projected read-only, with a bounded continuation authority (proposed candidate)

A new store, owned by `codegg-core`, keyed by `(run_id, ordinal)`, written by the same path that writes the child's messages and redacted at write time. The delegated run's identity remains `AgentRunId`; it never becomes a session and never enters the session catalog. A new bounded, redacted transcript DTO crosses the projection seam to a client bound to the owning session. Continuation reads the recorded transcript and starts a **new attempt of the same delegated run**, under the same authority, bounded to one continuation, authorized by the spawning principal.

Benefits: the artifact has exactly one owner; the user's session list is untouched; the projection carries an explicitly bounded and explicitly redacted payload; continuation is possible without minting any new identity; and the durable notice row required to make continuation reachable already exists under ADR-0031.

Costs and failure modes: a new table and migration; a new redaction surface that must be specified before it is written; a continuation path that is a new authority surface and therefore needs its own denial matrix; and the risk that a transcript is treated as an audit log, which it must not be.

### C. A real `session` row per delegated run, reusing the existing message store

Create a session row for the child and write its messages into the existing `message` table.

Benefits: reuses the message store, the session reader, the compaction path and the existing replay machinery wholesale; the child becomes an ordinary transcript with no new reader.

Costs and failure modes: this changes a canonical identity boundary — a delegated run becomes a session. `message.session_id` is a foreign key with `ON DELETE CASCADE`, so the session row is mandatory, and a session row is what makes a row appear in session enumeration, project binding, `/sessions`, `Ctrl+T` and the session-snapshot path that ADR-0043 and the live-work-visibility M004 already gave a precise meaning. It would also import the parent-session compaction and summarization policy into the child's history, and give the child a durable identity that survives in ways a delegated run's `run_id` should not. It is the smallest code change and the largest change of meaning.

## Proposed decision — subject to maintainer acceptance

**Option B**, with the following boundaries.

1. **Ownership and identity.** A delegated transcript is owned by `codegg-core` and keyed by `AgentRunId`. A delegated run is **not** a session: it has no `session` row, it never appears in session enumeration, project binding, `/sessions`, `Ctrl+T`, or any session-scoped snapshot, and it is never compacted by the parent session's policy. The synthetic `{parent}-sub-{task_id}` identifier remains an internal detail and is not a durable identity.

2. **Storage.** One additive migration adds the transcript store. A transcript row is `(run_id, ordinal, role, kind, redacted_body, byte_len, created_at)`. `role` distinguishes the orchestrator-visible channel from the child's internal channel; `kind` distinguishes a model message, a tool call, a tool result and a terminal note. Each body is redacted **at write time** — before it reaches the store — never at read time, so an unredacted body is never durable. Bounds are enforced per row and per run; a run whose transcript exceeds the per-run bound stops appending and records a truncation marker rather than silently losing the tail without saying so.

3. **What may cross the frontend.** A new bounded, redacted transcript DTO, delivered only to a subscribed client bound to the owning session. The projection seam is where this is enforced: the DTO declares its own byte bound, the existing projection guards must be extended to cover the new payload, and a payload that exceeds the declared bound is refused rather than truncated silently. Credentials, environment values, raw provider reasoning and unbounded tool output are **not** in the transcript body; tool output is bounded at capture, not at projection.

4. **Continuation authority.** A delegated run may be continued only by the principal that spawned it, under the parent's own authority and ceiling, and only while the run is a live or resumable terminal — never after a cancel, and never for a run whose parent is itself gone. A continuation is a **new attempt of the same delegated run** (`run_id` unchanged, attempt ordinal incremented), not a new run and not a new session. It is bounded to one continuation per terminal failure; a second failure is the parent's to handle by re-dispatching. A continuation performs no provider entry until the recorded transcript has been read back and redaction re-verified, so a continuation cannot smuggle an unreviewed prior context into a new provider request.

5. **Notification.** A delegated run's terminal state produces a **durable** notice row through the existing ADR-0031 machinery, with the disposition fixed to `NotifyOnly` — a notice, zero tokens, no turn minted, no session mutated. This ADR does not authorize `ResumeOwner` for delegated runs; a continuation requires the explicit authorization in (4), not a notice. The delegated job becomes a subscriber of `JobNotificationService` on the same terms as `bash` and `test`.

6. **What does not change.** ADR-0031, ADR-0041 and ADR-0043 are unchanged. Delegated run isolation, the writer fence, the worktree ceiling, the sandbox ceiling, the scheduler's admission authority, the tool broker's authority boundary and the `ToolBroker`-only dispatch rule are unchanged. No second scheduler, no second notice store, no delegated-run retry policy that the scheduler does not already own.

## Consequences

### Positive

- The orchestrator's knowledge of a delegated run becomes durable rather than best-effort: a parent turn that ends before its child no longer loses the child's terminal state, because the notice is a row and not a `try_send`.
- The user gains the artifact they asked for — a delegated run's actual conversation — as a bounded, redacted, projected transcript rather than a synthesized status line.
- A transient provider or upstream failure can cost a **continuation** rather than a full re-dispatch with a re-stated task and a re-created workspace.
- The delegated run's isolation, authority and writer fencing are untouched, and the delegated transcript never enters the user's session catalog.

### Negative

- A new table, a new migration, a new redaction surface and a new projection payload, each of which must be correct on the first attempt because a transcript is user-visible.
- A continuation is a new authority surface and needs its own denial matrix, its own audit and its own crash-safe exactly-once story — the same class of work ADR-0031 required for background jobs.
- The redaction rule becomes a standing obligation: every new tool output shape that reaches a transcript must be bounded and redacted at capture, or the transcript grows a hole.

### Neutral or deferred

- The legacy, non-durable delegated path is unaffected by this decision and is not authorized to change here; whether it is retired is a separate question.
- `Retryability::RequiresRecovery` remains defined for a git-conflicted repository. Whether a provider failure should also be classified `RequiresRecovery` is a *classification* question for the milestone that implements continuation, and it is explicitly **deferred** here: this ADR decides the authority, not the taxonomy.
- The 2 KiB in-band completion notice stays as-is. This ADR does not widen it; it makes a durable row the primary channel and leaves the in-band notice as the fast path.

## Compatibility and migration

- Additive only. One migration adds the transcript store; no existing table is rewritten and no existing row is removed.
- No `session` row is created for a delegated run, so the session catalog, the project catalog and the session-snapshot path are unchanged and no backfill is required.
- The projection DTO is additive and `#[serde(default)]`-tolerant, so an older client that does not know the transcript field continues to work against a newer daemon and renders the run inspector it already renders.
- `agent_run_result.summary` is unchanged and remains the orchestrator's fast-path summary. The `result_ref` pointer it is returned as is not deprecated by this ADR; making it dereferenceable is a milestone decision, not an ADR decision.
- A delegated run recorded before this decision has no transcript. Continuation for such a run is refused with a typed reason, not attempted; the milestone must prove that refusal.

## Security and reliability implications

- **Redaction at write.** A transcript body is redacted before it is stored. An unredacted body is never durable, so a later bug in a projection or a reader cannot expose it.
- **Bound enforcement.** Per-row and per-run byte bounds are enforced at write. A transcript cannot be used as an unbounded log, and truncation is recorded rather than silent.
- **Authority.** A continuation requires the spawning principal. It runs under the parent's ceiling, sandbox and worktree. It cannot exceed the parent's tool policy, cannot widen its own `allowed_paths`, and cannot mint a session or a turn.
- **Exactly-once.** A continuation is bounded to one per terminal failure and is crash-safe: a crash after the provider entry is recorded as needing attention, never blindly replayed. The pattern is the one already proven in `src/agent/job_notification_recovery.rs`.
- **Denial matrix.** Absent, expired, mismatched, cancelled, superseded, wrong-principal, wrong-parent and already-continued cases must all fail closed before any provider entry, with a typed reason that the orchestrator can read.
- **The projection seam is a trust boundary.** The new transcript payload extends `check_projection_disclosure.sh`, `check_projection_publication_seam.sh`, `check_projection_transport_isolation.py` and `check_websocket_bounds.py`. A declared-but-unwritten field is a build failure, not a default.

## Verification and acceptance gate

Acceptance of this ADR is a maintainer decision recorded in this file. It is **not** acceptance of an implementation: filing it grants no capability, and the milestones that depend on it stay `blocked` until it is accepted.

When accepted, the following evidence is required from the implementing milestones:

- A durable transcript store whose redaction is proven by a test that writes a known-secret-shaped body and proves the secret is absent from the stored row — not merely absent from the rendered view.
- A projected transcript whose byte bound is proven by a test that exceeds the bound and observes a refusal or a recorded truncation, never a silent drop.
- A continuation path whose every denial case is proven to fail before any provider entry, and whose exactly-once property is proven across a crash injection at each boundary.
- A static guard with a `--self-test` that fails when the delegated transcript's declared projection fields are not written in production — the same non-vacuity standard the repository applies to its other guards.

## Supersession

None. This record adds to ADR-0031, ADR-0041 and ADR-0043 and amends none of them. If it is accepted and later superseded, the superseding ADR must carry the storage, redaction and continuation boundaries forward explicitly.
