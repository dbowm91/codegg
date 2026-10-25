# Delegated Run Observability and Recovery Corrective Milestone 004 — A Delegated Run Can Be Continued

Status: blocked — C002 is not closed, and ADR-0051 is proposed, not accepted

Repository baseline: `a21b5fffa` on `fix/tui-composer-helix-parity-post-closure-corrective` (2026-10-24). Revalidate the actual implementation head before editing.

Source roadmap: `plans/subsystems/delegated-run-observability-and-recovery-corrective-addendum.md` §7 (C004)

Long-term requirements:

- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md` — agent run, delegated run, attempt

Applicable ADRs:

- `plans/adrs/ADR-0051-durable-delegated-run-transcript-and-continuation-authority.md` — **proposed; this milestone's gate.** §4 fixes the continuation authority: the spawning principal only, under the parent's authority and ceiling, one continuation per terminal failure, a new attempt of the same run, and no provider entry until the recorded transcript has been read back and redaction re-verified.
- `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md` (accepted) — the agent loop's evidence-gated recovery; this milestone adds a *delegated-run-level* continuation and does not change the loop's own ladder.
- `plans/adrs/ADR-0041-subagent-run-addressing-and-human-interaction-authority.md` (accepted) — unchanged; a continuation is initiated by the **orchestrator**, not the human.

Primary class: capability

## 1. Objective

Make a failed delegated run **continuable** by the principal that spawned it, instead of disposable.

One bounded outcome: when a delegated run fails for a recoverable reason, the orchestrator can continue that **same** run — a new attempt of the same run id, seeded from the recorded transcript, under the parent's own authority and ceiling — and every case in which it must not happen fails closed before any provider entry.

## 2. Why this milestone is ready

**It is not.** This milestone is **blocked** on two named dependencies:

1. **C002 is not closed** — `plans/implementation/delegated-run-observability-and-recovery-corrective/002-a-durable-transcript-for-a-delegated-run.md`. A continuation is a continuation *of a conversation*; without the durable transcript there is nothing to continue from, and a "continuation" that starts from nothing is a re-dispatch with a different name.
2. **ADR-0051 is not accepted** — `plans/adrs/ADR-0051-durable-delegated-run-transcript-and-continuation-authority.md`. Its §4 is the authority contract this milestone implements, and its §"Neutral or deferred" explicitly defers the provider-failure **retryability taxonomy** to this milestone (roadmap D3), which is a decision this milestone must make and record.

**Soft dependency:** C001. C001 gives the orchestrator the durable notice that tells it the child failed. Without it the orchestrator may never learn that a continuation is warranted. C001 must close before this milestone's closure.

## 3. Current implementation evidence

### 3.1 There is no continuation action

`src/tool/task.rs:1278` — the action enum is `"scratch_create", "scratch_revoke", "spawn", "converge", "convergence_status", "convergence_decide", "convergence_cancel", "spawn_many", "create_group", "status", "get", "message", "interrupt", "wait", "cancel", "status_group", "wait_group", "cancel_group", "board", "claim", "release_claim", "request_handoff"`. There is no `resume` and no `continue`.

`task action=message` (`src/tool/task.rs:1765-1783`) reaches a **live** run only: `src/agent/run_control.rs:229-231` returns `Terminal` for a run that has already finished, and a message to a terminal run is superseded. So the existing message path cannot continue a failed child.

### 3.2 The delegated job is not retryable

`src/tool/task.rs:2096-2097` — `retry_policy: codegg_core::jobs::RetryPolicy::no_retry()`, `idempotency: codegg_core::jobs::IdempotencyClass::NonIdempotent`. `retry_job` is refused, and daemon-restart requeue refuses non-idempotent jobs. The only retry lever today is an operator-driven `CoreRequest::JobRetry`.

### 3.3 The retryability taxonomy does not describe a provider failure

`src/scheduler/executors.rs:1724-1729` — `Retryability::RequiresRecovery` is set **only** for `RepositoryState::Conflicted`; `success` is `NotRetryable`; everything else is `Retryable`. A provider or upstream failure therefore falls into the generic `Retryable` bucket, which nothing consumes for a delegated run.

### 3.4 A provider error inside the child's own turn is retried in-process

`src/agent/provider_turn.rs` retries transient provider errors inside a single provider turn with a bounded attempt count and jittered backoff; permanent errors (authentication, model-not-found, an open circuit) are not retried. The gap this milestone closes is the one **above** that: a child whose turn budget, tool budget, deadline or provider circuit has ended, and which therefore needs a *new attempt* rather than another retry inside the same turn.

### 3.5 The live-run machinery already supports addressing a running child

`src/agent/worker.rs:1690-1700` registers the child's `LiveRunHandle` with the run-control service, wiring the child's `follow_up_sender()` as the handle's `follow_up_tx` and the child's steer receiver as `steer_tx`. `src/agent/run_control.rs:183-218` (`authorize`) already decides whether an actor may address a run: the actor's session and turn must match the task's originating session and turn and the run must be a direct child, or the actor must be the run's parent. That is the authority rule a continuation must reuse, not replace.

### 3.6 Why the existing verification passed

No test asserts that a failed delegated run can be continued, because the capability does not exist and its absence was never a check's subject.

## 4. Invariants that must not regress

- **A continuation is a new attempt of the same run, not a new run and not a new session.** The `run_id` is unchanged; an attempt ordinal is incremented.
- **The continuation runs under the parent's authority.** The parent's tool policy, denied tools, `allowed_paths`, sandbox ceiling, worktree ceiling and model are exactly the ones the original attempt had. A continuation may not widen any of them, and may not choose a different model.
- **Only the spawning principal may continue.** The rule at `src/agent/run_control.rs:183-218` is reused, not relaxed.
- **One continuation per terminal failure.** A second failure is the parent's to handle by re-dispatching. A continuation is claimed by a compare-and-swap; exactly one winner under duplicates.
- **No provider entry before the transcript is read back and redaction re-verified.** A continuation that seeded a provider request from an unreviewed prior context would be a smuggling path.
- **No provider entry before every denial case has been evaluated.** Every denial fails closed *before* inference, with a typed reason the orchestrator can read.
- **A cancelled run is never continued.** Cancellation is a decision, not a failure.
- **The delegated job's retry policy is not changed to make this work.** The continuation is a new attempt under an explicit authorization, not the scheduler retrying a `no_retry` job.
- **ADR-0040's own recovery ladder is unchanged.** This milestone adds a delegated-run-level continuation; it does not alter the loop's evidence-gated stall recovery.
- **Crash safety.** A crash after the provider entry is recorded as needing attention, never blindly replayed — the pattern already proven in `src/agent/job_notification_recovery.rs`.

## 5. Scope

### In scope

- A `task` action that continues a failed delegated run, keyed by the run id.
- The authorization rule, reusing `src/agent/run_control.rs:183-218`.
- Seeding the new attempt from the C002 transcript.
- The attempt ordinal and its persistence.
- The compare-and-swap claim that makes the continuation exactly-once.
- The denial matrix, with a typed reason per case.
- The classification decision that roadmap D3 defers: **which failures are continuable**. The milestone must record the decision and its reason.
- The orchestrator's copy: the tool description must state that a continuation exists, when it is warranted, and that it is bounded.
- The affected `architecture/` and `docs/` statements.

### Explicitly out of scope

- **Any change to the delegated run's transcript store.** C002 owns it.
- **Any change to the TUI's delegated tab.** C003 owns it.
- **Any human-initiated continuation.** ADR-0041 stands: the human steers the parent, and the parent decides whether to continue its child.
- **Changing the delegated job's `RetryPolicy` or `IdempotencyClass`.**
- **A general agent-resume facility** for parent sessions, the queue, or any other surface. This milestone is about delegated runs only.
- **Re-running a cancelled run.**
- **Widening the parent's model, tools, paths, sandbox or worktree on a continuation.**
- **The `Retryability` taxonomy beyond the delegated-run case.** The milestone decides which delegated failures are continuable; it does not redesign the shared enum.
- **A cross-run continuation** — continuing run A's conversation inside run B.

## 6. Required production changes

### Core/domain

- **The attempt.** A delegated run gains an attempt ordinal and a record of the attempt's outcome. The attempt is the unit a continuation creates; the run is the unit the orchestrator addresses.
- **The continuation authorization.** A typed record of who may continue which run, under which parent's authority, bounded to one per terminal failure. It must be claimable exactly once.

### Storage and migrations

- One additive migration for the attempt record and the continuation authorization, if C002's migration does not already carry them. **Two migrations must not be added for one milestone** — if C002's migration is still open when this milestone starts, the two are merged, and the merge is recorded as a deviation.
- The attempt record is append-only, like the transcript.

### Protocol and DTOs

- A new `task` action — `continue` — with a schema consistent with the existing `status`/`wait`/`message`/`interrupt`/`cancel` actions. It takes a run id.
- The action's result states: the attempt ordinal, the run id, whether the continuation was accepted or refused, and — when refused — the typed reason.
- The tool's own description (`src/tool/task.rs:1268`) must state the new capability, its bound, and when it is warranted. It currently tells the model nothing about continuing a failed child.

### Runtime and concurrency

- **The continuation flow**, in order, each step before any provider entry:
  1. **Resolve** the run by run id and read its terminal outcome and its parent.
  2. **Authorize**: the actor must be the spawning principal, per `src/agent/run_control.rs:183-218`'s rule. A run with no live parent, a cancelled run, a superseded run, an already-continued run, an expired authorization and a wrong-principal actor all fail here.
  3. **Classify**: the terminal failure must be one the milestone's recorded decision designates as continuable. A provider authentication failure is not; a provider rate-limit or transient-upstream failure may be.
  4. **Claim**: a compare-and-swap claims the continuation. Exactly one winner.
  5. **Read back** the recorded transcript and **re-verify** its redaction — the seeded context must be exactly what was stored.
  6. **Seed and dispatch**: a new attempt under the parent's authority, with the attempt ordinal incremented.
- **Exactly-once.** A duplicate continuation request is refused, not replayed.
- **Crash safety.** A crash between the claim and the dispatch leaves the continuation claimed and not dispatched; the next reconciliation must surface it as needing attention, never blindly replay it. The failpoint pattern in `src/agent/job_notification_recovery.rs:249` is the model.
- **The parent's own turn** observes the continuation's outcome through the same durable notice path C001 establishes. A continuation must not create a second, parallel notification channel.

### Frontend or operator surface

None required. The orchestrator's transcript shows the continuation's outcome through the existing notice path. The TUI's delegated tab is unchanged (C003 owns it); if C003 has closed by then, the tab renders the new attempt like any other attempt.

### Security and authorization

- **The continuation inherits the parent's ceiling, and cannot widen it.** The parent's `denied_tools`, `allowed_paths`, sandbox profile, worktree ceiling and model are carried through unchanged. A continuation that widened any of them would be a privilege escalation.
- **The seeded context is exactly the stored, redacted transcript.** It is read from the store, not reconstructed from a live in-memory channel, so a continuation cannot inherit an unreviewed context.
- **Denial before inference.** Every denial case must be proven to fail before any provider entry.
- **Bounded.** One continuation per terminal failure. The attempt ordinal makes the bound observable.
- **Audit.** Each continuation is recorded: who continued which run, under which authority, with which outcome. The audit is not a transcript and does not hold the conversation.

### Documentation and static guards

- `architecture/agent.md` — what a continuation is, what it inherits, and what it may not do.
- `architecture/jobs.md` — the attempt ordinal and the exactly-once claim.
- `architecture/permission.md` — if the continuation touches a permission surface, the rule must be stated there rather than implied.
- `src/tool/task.rs:1268` — the tool's own description.
- **Static guard (required).** A check that every path that starts a delegated run's provider entry either is the original attempt or is a claimed continuation. It fails when a provider entry for a delegated run is reachable without a claimed continuation. It carries a `--self-test` and is wired into both `scripts/verify.sh` and `.github/workflows/ci.yml`.

## 7. Ordered work packages

### Work package A — Decide and record what is continuable

Intent: roadmap D3 is deferred to this milestone; the decision must be made explicitly, not inherited from a generic bucket.

Required changes: decide which terminal failures of a delegated run are continuable, and write the decision and its reason into the plan's closure record and the affected `architecture/` document. The decision must distinguish at minimum: a provider authentication or authorization failure (not continuable — retrying it cannot succeed), a provider rate-limit or transient upstream failure (continuable), a deadline or budget exhaustion (continuable), a tool-permission denial (continuable — the parent may re-dispatch with different authority), and a cancellation (never continuable).

Acceptance evidence: the recorded decision, and a test per case.

### Work package B — The authorization and the claim

Intent: exactly one continuation, by the right principal.

Required changes: per §6 "Runtime and concurrency" steps 1–4.

Acceptance evidence: the full denial matrix, each case failing closed before any provider entry, each with a typed reason.

### Work package C — Seeding from the transcript

Intent: the new attempt continues the conversation.

Required changes: per §6 "Runtime and concurrency" step 5.

Acceptance evidence: a test that the seeded context equals the stored transcript; a test that the redaction is re-verified at read-back and that a tampered row is refused rather than seeded.

### Work package D — The attempt and the dispatch

Intent: a new attempt of the same run, under the parent's authority.

Required changes: per §6 "Runtime and concurrency" step 6, plus the attempt ordinal's persistence.

Acceptance evidence: a test that the run id is unchanged, the attempt ordinal increments, and the parent's ceiling is byte-identical to the original attempt's.

### Work package E — Crash safety and exactly-once

Intent: no duplicate continuation, no blind replay.

Required changes: per §6 "Runtime and concurrency".

Acceptance evidence: a duplicate-request test; a crash-injection test at each boundary that proves the outcome is "needs attention" rather than a replay.

### Work package F — Docs, guard and closure

Intent: the repository states the truth and cannot drift back.

Required changes: the documentation updates in §6; the guard with its `--self-test`, wired into `scripts/verify.sh` and `.github/workflows/ci.yml`.

Acceptance evidence: `bash scripts/verify.sh quick` exits 0; the guard fails against the pre-change tree.

## 8. Failure, cancellation, restart, and contention semantics

- **The continuation is requested twice.** Exactly one is accepted; the other is refused with a typed reason. Never two attempts.
- **The daemon crashes between the claim and the dispatch.** The continuation is claimed and not dispatched. The next reconciliation surfaces it as needing attention; it does **not** replay.
- **The daemon crashes after the provider entry.** Same rule: needing attention, never blind replay. The failpoint pattern at `src/agent/job_notification_recovery.rs:249` is the model.
- **The child's transcript is truncated.** The continuation is seeded from what is recorded, and the seeded context says it is truncated. A continuation must not silently invent the missing tail.
- **The transcript is absent** (a run recorded before C002). The continuation is **refused** with a typed reason. It must not start from an empty context and present itself as a continuation.
- **The parent's turn has ended.** The continuation is authorized by the principal, not by a live turn. A continuation may outlive the turn that requested it, and must be claimed exactly once regardless.
- **The parent has been cancelled.** No continuation.
- **The provider authentication fails again.** The second failure is not continuable; the parent is told so and must re-dispatch differently or escalate.
- **Two parents attempt to continue the same child.** Only the spawning principal wins; the other is refused.
- **A concurrent append to the transcript while the continuation reads it.** The read is a consistent page.

## 9. Compatibility and migration

- **Additive.** One migration (merged with C002's if C002's is still open).
- **Additive tool surface.** A new action on the `task` tool. A model that never uses it is unaffected.
- **No protocol change** to the projection, the delegated lifecycle, or the session catalog.
- **No change to the delegated job's `RetryPolicy` or `IdempotencyClass`.**
- **A run recorded before this milestone has no attempt record.** Its attempt ordinal reads as 1, and a continuation of it is refused because it has no transcript (see §8).
- **Older clients** are unaffected: the new action is a tool action, and the notice path is C001's.

## 10. Required tests

### 10.1 Focused unit tests — the authorization and the claim

- The spawning principal may continue; a different principal is refused.
- A cancelled run is refused.
- An already-continued run is refused.
- A second concurrent request is refused; exactly one attempt is created.
- A non-continuable failure (per work package A's decision) is refused.

### 10.2 Integration tests — `tests/agent_loop/`

- A continuable failure is continued: the run id is unchanged, the attempt ordinal increments, and the new attempt's context equals the stored transcript.
- The continued attempt inherits the parent's ceiling **byte-identically**: same denied tools, same allowed paths, same sandbox profile, same worktree ceiling, same model.
- The parent's turn observes the continuation's outcome through the same durable notice path — no second channel.
- A run with no transcript is refused with a typed reason.
- A truncated transcript is continued from what is recorded, and the seeded context says so.
- A tampered transcript row is refused rather than seeded.

### 10.3 Restart and crash tests

- A crash at each boundary between the claim and the dispatch produces "needs attention", never a replay and never a duplicate attempt.

### 10.4 Security and negative tests

- The full denial matrix, each case proven to fail **before** any provider entry.
- A continuation cannot widen the parent's ceiling: an attempt to do so is refused.
- The seeded context contains nothing that was not redacted at write.

## 11. Required verification commands

```bash
# narrow first
cargo test -p codegg-core -- agent_run
cargo test -p codegg --lib -- run_control
cargo test --test agent_loop -- <member>
cargo test --test scheduler_jobs -- <member>

# static guards
bash scripts/check-core-boundary.sh
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py

# formatting and linting
cargo fmt --all
CARGO_BUILD_JOBS=8 cargo clippy --workspace --all-targets --locked -- -D warnings

# the canonical gate
bash scripts/verify.sh quick
```

`bash scripts/verify.sh quick` is the canonical gate and must exit 0. Never run `cargo test --workspace`, a bare `cargo test`, or anything with `--all-features`.

## 12. Documentation updates

- `architecture/agent.md` — what a continuation is, what it inherits, and what it may not do; the recorded decision on which failures are continuable.
- `architecture/jobs.md` — the attempt ordinal and the exactly-once claim.
- `architecture/permission.md` — only if the continuation touches a permission surface; state the rule there rather than implying it.
- `architecture/scheduler.md` — that a continuation is a new attempt under an explicit authorization, **not** the scheduler retrying a `no_retry` job.
- `src/tool/task.rs:1268` — the tool's own description: the new action, its bound, and when it is warranted.
- `docs/tui.md` — only if the delegated tab's user-facing description mentions what the user can do with a failed child.

## 13. Acceptance criteria

1. A continuable delegated failure can be continued by the spawning principal, as a **new attempt of the same run**, with the attempt ordinal incremented.
2. The continued attempt inherits the parent's ceiling exactly: tools, paths, sandbox, worktree and model.
3. The seeded context is exactly the stored, redacted transcript, read back and re-verified before any provider entry.
4. Every denial case — wrong principal, cancelled, superseded, already-continued, absent transcript, non-continuable failure — fails closed **before** any provider entry, with a typed reason.
5. Exactly one continuation succeeds under duplicate and concurrent requests.
6. A crash at each boundary produces "needs attention", never a replay and never a duplicate attempt.
7. The orchestrator learns the continuation's outcome through the same durable notice path C001 established; no second channel exists.
8. The delegated job's `RetryPolicy` and `IdempotencyClass` are unchanged.
9. The decision on which failures are continuable is recorded, with its reason, in the closure record and in `architecture/agent.md`.
10. The static guard fails against the pre-change tree and passes after, with its `--self-test` proving non-vacuity.
11. `bash scripts/verify.sh quick` exits 0 and `cargo clippy --workspace --all-targets --locked -- -D warnings` is clean.

## 14. Stop conditions

The implementing agent must stop and report rather than improvise when:

- **C002 is not closed**, or **ADR-0051 is not accepted** — the milestone's gate.
- the continuation cannot be authorized without relaxing `src/agent/run_control.rs:183-218`'s rule;
- the seeded context cannot be re-verified against the stored transcript — seeding an unreviewed context is a smuggling path, not a trade-off;
- making the continuation work would require changing the delegated job's `RetryPolicy` or `IdempotencyClass`, or would require the scheduler to retry a `no_retry` job;
- the decision on which failures are continuable turns out to require a shared taxonomy change — that is a separate decision, and the milestone must record it as out of scope rather than widen the enum;
- the change would expand into C003's TUI scope or into a general agent-resume facility;
- the continuation would need a human-initiated path — ADR-0041 forbids it, and if it appears to be needed, the design is wrong.

## 15. Closure evidence required

The closure record at `plans/closure/delegated-run-observability-and-recovery-corrective/004-status.md` must contain:

1. The C002 closure reference and ADR-0051's acceptance commit.
2. The requirement-to-evidence matrix for §13, each row naming a **file:line** and a named test.
3. **The recorded continuability decision**, with its reason and its per-case table.
4. **The denial matrix**, each case quoted with its typed reason, and each proven to fail before any provider entry.
5. **The exactly-once evidence**: the duplicate-request test and the crash-injection tests, quoted.
6. **The inheritance evidence**: a test proving the continued attempt's ceiling is identical to the original's, quoted.
7. The exact commands run and their real results, labelled local versus CI.
8. The guard's `--self-test` output and its failure against the pre-change tree.
9. An invariant review against §4, item by item.
10. A failure-and-recovery review against §8.
11. Any deviation from this plan, recorded as a deviation — in particular any merge of C002's migration with this milestone's.
12. The registry updates and the **unblock audit**.

## 16. Handoff notes

- **Blocked** on C002 and ADR-0051.
- **Reuse, do not replace, the existing authorization rule.** `src/agent/run_control.rs:183-218` already decides who may address a delegated run. A continuation is an instance of that question, not a new one.
- **Do not widen the parent's ceiling.** A continuation that changes the parent's tools, paths, sandbox, worktree or model is a privilege escalation, and the test in §13.2 must prove the opposite.
- **Do not change the delegated job's retry policy.** The continuation is an explicitly authorized new attempt, not the scheduler retrying.
- **Fail closed, always, before inference.** The denial matrix is the milestone's core, not an appendix.
- **Test conventions.** Root integration tests are **families**: `cargo test --test <family> -- <member>`. `tests/agent_loop/subagent.inc` is the delegated-run member file.
- **New `#[tokio::test]`s** default to `current_thread`; use `flavor = "multi_thread", worker_threads = 2` only for real concurrency or subprocesses.
- **Assertion quality.** Every new test body must assert a checkable property or delegate to a named helper that does, or be recorded in `docs/test-assertion-quality.toml`. `scripts/check_test_assertion_quality.py` and `scripts/check_test_duplication.py` both run in `verify.sh quick`.
- **Scan Rust with `scripts/rust_test_scan.py`**, never with a regex.
- **Budget.** `CARGO_BUILD_JOBS=3` while iterating; 8 for the final workspace clippy.
- **Comment budget.** `scripts/check_comment_budget.py` is a ratchet: re-bank **after** `cargo fmt --all`.
- **Guard wiring.** The new guard must appear in **both** `scripts/verify.sh`'s `run_guards()` and `.github/workflows/ci.yml`.
