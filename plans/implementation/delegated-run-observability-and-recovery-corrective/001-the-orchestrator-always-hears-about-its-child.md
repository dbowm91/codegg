# Delegated Run Observability and Recovery Corrective Milestone 001 — The Orchestrator Always Hears About Its Child

Status: ready for handoff

Repository baseline: `a21b5fffa` on `fix/tui-composer-helix-parity-post-closure-corrective` (2026-10-24). Revalidate the actual implementation head before editing.

Source roadmap: `plans/subsystems/delegated-run-observability-and-recovery-corrective-addendum.md` §7 (C001)

Long-term requirements:

- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#28-observability`
- `plans/001-terminology-and-domain-model.md` — agent run, delegated run, session

Applicable ADRs:

- `plans/adrs/ADR-0031-authenticated-idle-continuation-delegated-turn-admission.md` (accepted) — the durable notice and continuation machinery this milestone subscribes to. **Not amended by this milestone.**
- `plans/adrs/ADR-0041-subagent-run-addressing-and-human-interaction-authority.md` (accepted) — unchanged.
- `plans/adrs/ADR-0043-session-owned-subagent-lifecycle-projection.md` (accepted) — unchanged.
- `plans/adrs/ADR-0051-durable-delegated-run-transcript-and-continuation-authority.md` (proposed) — **not required by this milestone**; this milestone deliberately stays inside ADR-0031's accepted contract. It is recorded here so the milestone's boundary against C002–C004 is explicit.

Primary class: capability

## 1. Objective

Make the orchestrator's knowledge of a delegated run **durable** rather than best-effort, and make the child's recorded result **readable** by the orchestrator.

Concretely, one bounded outcome: a delegated job is a subscriber of the daemon's existing durable job-terminal-notification service with the disposition fixed to `NotifyOnly`, so every delegated run's terminal state becomes a durable notice row that the next parent turn can read; and the `task` tool hands the orchestrator the child's recorded result instead of an opaque pointer.

The milestone adds no new store, no new authority and no new identity. It reuses machinery that already ships for `bash` and `test`.

## 2. Why this milestone is ready

- **Hard dependencies: none.** ADR-0031 is accepted and its machinery is shipped: `JobNotificationService` (`src/scheduler/job_notifications.rs`), the durable `JobBackgroundSubscription` and `JobTerminalNotice` tables, the `JobCompletionDisposition` enum (`src/scheduler/job_notifications.rs:197-202`), and the crash-safe recovery loop `src/agent/job_notification_recovery.rs`.
- **The scheduler already records terminal notices generically.** `src/scheduler/scheduler.rs:437-461` (`notify_job_terminal`) calls `service.record_terminal(terminal_job, summary, run_id)` for **any** subscribed job; unsubscribed jobs return silently. No scheduler change is required to make a delegated job produce a notice — only a subscription.
- **The durable delivery point already exists and already runs per turn.** `src/agent/follow_up.rs:420-437` calls `inject_recoverable_job_notifications` at the turn boundary and turns each pending notice into one durable session event plus one `Message::System`. Its own comment states the contract: *"every notice becomes one durable session event plus one system message at the next turn."*
- **The delegated job already carries the parent's session and turn.** `src/tool/task.rs:2049-2051` sets `session_id: self.parent_session_id.clone()` and `turn_id: self.parent_turn_id.clone()` on the `NewJob`, which is exactly what `subscribe_with_options` requires for exact-owner routing (`src/scheduler/job_notifications.rs`, `subscribe_with_options` refuses an empty `session_id`).
- **Interface dependencies are stable**: `JobNotificationService::subscribe_with_options`, `JobCompletionDisposition`, and `inject_recoverable_job_notifications` are all accepted, tested surfaces.

## 3. Current implementation evidence

### 3.1 The delegated job is not a subscriber

`src/tool/task.rs` never calls `subscribe_with_options`. The two existing subscribers are `src/tool/bash/process.rs:1614` and `src/tool/test.rs:447`. `src/tool/test.rs:443-459` is the model to follow, including its ordering comment:

> "Durable subscription before reporting acceptance. If the daemon restarts after submit but before subscribe, retry with the same invocation key returns the same job and completes the subscribe."

`src/tool/test.rs` receives its `JobNotificationService` handle as a tool option; the `task` tool has no equivalent handle today and must be given one.

### 3.2 The orchestrator's current terminal knowledge is a best-effort push

`src/agent/run_control.rs:406-443` pushes `"Child run {run_id} finished with status {status}. Summary: {summary}"` into the parent's `follow_up_tx`, bounded to 2 KiB (`src/agent/run_control.rs:16`). The push is a `try_send` on a bounded channel and is **dropped** when the channel is full or closed (`src/agent/run_control.rs:414-420`, warn-only). Separately, `src/agent/turn_runtime.rs:1124` unregisters the parent's live turn when the turn ends, so a child that finishes afterwards matches no live target and its terminal state reaches nobody.

The notice is drained once per `run()` at `src/agent/loop.rs:2176`; `src/agent/follow_up.rs:456-476` turns it into a `Message::User`.

### 3.3 The child's result is returned as an unreadable pointer

`src/scheduler/executors.rs:1464` stores `result_ref = agent-run-result:{run_id}`. `src/tool/task.rs:1819-1831` returns `Result: <that pointer>` to the model. The action enum at `src/tool/task.rs:1278` has no action that dereferences it. `agent_run_result.summary` already holds the child's recorded output.

### 3.4 The legacy path has no push at all

`src/agent/worker.rs:814-860` updates only the in-memory `TaskStore` on the legacy (non-durable) path, so the orchestrator must poll. `src/tool/task.rs:1915` selects the durable path only when `self.agent_runs` is `Some`; the daemon wires it (`src/core/daemon_construct.rs:555`), and the fallback branch exists.

### 3.5 Why the existing verification passed

The delegated-run notice path was never exercised end-to-end: `src/agent/job_notification_recovery.rs`'s own tests cover only `expected_event_is_stable` (`src/agent/job_notification_recovery.rs:427-441`). There is no test that a delegated run produces a durable notice, and none that a parent turn which ended before its child still learns the outcome.

## 4. Invariants that must not regress

- **A delegated run's authority is its parent's.** The subscription grants the notice, nothing more. It does not grant a turn, a session, a provider entry, a wider tool set, a wider path set, a wider sandbox or worktree ceiling.
- **`NotifyOnly` spends no tokens.** A notice must not synthesize, reopen or extend a model turn by itself. ADR-0031's `resume_owner` disposition is **not** authorized for delegated runs by this milestone.
- **The scheduler remains the only admission authority.** The delegated job is still submitted through `JobSubmissionService`; this milestone adds a subscription after submission, never a second submission path.
- **A notification failure never fails the delegated work.** `src/scheduler/scheduler.rs:437-461` already treats notice persistence as non-fatal; the subscription must obey the same rule, or the spawn is at risk of failing for a bookkeeping reason.
- **The durable notice store remains the only notice store.** No second notice mechanism, no new table.
- **Exact-owner routing.** A notice reaches the session that owns the delegated job and no other (`src/agent/job_notification_recovery.rs:118-126`).
- **Delegated run isolation, the writer fence, the worktree ceiling and the `ToolBroker`-only dispatch boundary are unchanged.**
- **The session catalog is unchanged.** No `session` row is created and no session enumeration changes.
- **The legacy delegated path is not modified** beyond what the scope section below explicitly permits.

## 5. Scope

### In scope

- Give the `task` tool a `JobNotificationService` handle and wire it from the daemon on the same terms as `src/tool/test.rs`.
- Subscribe the delegated job after a successful submit and before the tool reports acceptance, with the disposition fixed to `NotifyOnly`, an `invocation_key` equal to the submission key, the parent's session and turn, the parent's workspace, and a `kind` token of `"subagent"`.
- Make the delegated terminal notice carry the **run id** and a summary the orchestrator can act on, without fabricating content.
- Make the child's recorded result readable by the orchestrator through the `task` tool, bounded, with the existing `result_ref` retained.
- Decide, with evidence, whether the in-band `try_send` push remains, and make its loss non-authoritative in the code and in the logs.
- Establish and document which delegated path is live in production, and scope the guarantee honestly to it.
- Update the affected `architecture/` and `docs/` statements, and add one static guard with a `--self-test`.

### Explicitly out of scope

- **Any durable transcript.** C002 owns that, behind ADR-0051. This milestone does not write, store or project a child's conversation.
- **Any continuation or resume.** C004 owns that, behind ADR-0051. `ResumeOwner` is not used here.
- **Any change to the delegated run's transcript, identity, worktree, sandbox or tool policy.**
- **Any change to ADR-0031, ADR-0041 or ADR-0043.**
- **Any change to the legacy delegated path's behaviour**, beyond documenting what it does and does not guarantee.
- **Retiring the legacy path.**
- **Any change to the `agent_runs` projection DTO or the TUI's run inspector.**
- **A retry policy for the delegated job.** The delegated job stays `RetryPolicy::no_retry()` + `IdempotencyClass::NonIdempotent` (`src/tool/task.rs:2096-2097`).
- **The provider-failure retryability taxonomy** (roadmap D3).

## 6. Required production changes

### Core/domain

None. `codegg-core`'s domain types are unchanged by this milestone.

### Storage and migrations

None. The `job_background_subscription` and job-notice tables already exist and already serve `bash` and `test`. **A migration is not required, and adding one is a scope violation** — if one appears necessary, stop and report (see §14).

### Protocol and DTOs

No wire-protocol change is required. The notice already carries `job_id`, `kind`, `summary`, `status` and `run_id` (`src/scheduler/job_notifications.rs`, `record_terminal(terminal_job, summary, run_id)`).

Two content changes are in scope:

1. **The notice names the delegated run.** `format_job_notice_content` (`src/scheduler/job_notifications.rs:1797-1826`) currently emits `"Background job {job_id} (kind {kind}) completed successfully: {summary}"`. For a delegated run the orchestrator must be able to tell *which child* the notice is about and *what to do next*. Either the delegated `kind` gets a dedicated content format, or the summary is required to carry the run id and the result reference. Choose one and record the choice; the requirement is that a reader of the notice can identify the run.
2. **The summary is real.** The summary must be the delegated run's recorded outcome. If the delegated job's terminal summary is currently empty or generic, fix it to carry the recorded outcome. **A notice must never report a success the run did not have**, and a delegated run whose output is unavailable must say so rather than read as empty-but-successful.

### Runtime and concurrency

#### 6.1 The subscription

After `submission.submit(...)` succeeds for a delegated job, and **before** the tool reports acceptance:

```rust
notifications.subscribe_with_options(
    &submitted.job_id,
    "subagent",                              // stable kind token
    parent_session_id,                       // non-empty, host-originated
    parent_turn_id.as_deref(),               // the originating turn
    invocation_key,                          // == the SubmissionKey used for submit
    JobCompletionDisposition::NotifyOnly,    // fixed; ResumeOwner is not authorized here
    owner_principal.as_deref(),
    Some(workspace_id.as_str()),
    None,                                     // ttl: the service's default
)
```

Rules, each of which the implementing agent must honour:

- **Ordering.** Subscribe **after** the submit succeeds and **before** the tool's success result is returned. The `src/tool/test.rs:443-459` comment states the reason: a daemon restart between submit and subscribe is repaired by retrying with the same invocation key, which returns the same job and completes the subscription.
- **Idempotency.** `subscribe_with_options` is idempotent for the same `(session_id, invocation_key)` and returns a `Conflict` for a different owner. The implementation must treat an existing-matching subscription as success, and a **conflict as a failure of the spawn** — a retry with a *different* parent must not silently attach a child's terminal notice to the wrong session.
- **`invocation_key` identity.** It must be the same key used for `SubmissionKey::new(key)` at submit time (the delegation key on the durable path), so the subscribe is idempotent under a retried spawn.
- **Empty parent session.** `subscribe_with_options` refuses an empty `session_id` ("background jobs require a real host-originated session context"). A delegated spawn with no host-originated parent session must **skip** the subscription and log the reason; it must **not** fail the spawn and must **not** fabricate a session id.
- **Failure isolation.** A subscription failure must not fail the delegated work. It must be logged at `warn` with the job id and the reason, and the orchestrator's fallback is the existing in-band push. This mirrors `notify_job_terminal`'s non-fatal rule (`src/scheduler/scheduler.rs:437-461`).
- **Disposition.** `NotifyOnly`. Not `ResumeOwner`, not `Suppress` — `Suppress` would silence the notice entirely, which is the opposite of this milestone's purpose.

#### 6.2 The in-band push

`src/agent/run_control.rs:406-443` keeps its `try_send`. After this milestone the **durable notice row is the authoritative channel** and the in-band push is a fast path. The implementation must make that relationship explicit in the code's own words, so a future reader does not mistake the push for the guarantee, and must update the drop log at `src/agent/run_control.rs:414-420` to say that the durable notice is unaffected.

The push must not be removed in this milestone: it is what delivers the notice to a **live** parent turn without waiting for the next boundary, and removing it would regress turn latency.

#### 6.3 Reading the child's recorded result

`task action=get` (the durable branch at `src/tool/task.rs:1795-1831`) currently returns `Result: agent-run-result:<run_id>`. It must additionally return the child's recorded result, read from the durable run-result store and bounded by the store's own bound.

- The existing `result_ref` line is **retained** so an existing caller is not broken.
- The result is returned under a clearly named field, not appended to a free-form string, so its bound is observable.
- When the run has **no recorded result**, the tool says so plainly. An empty string that reads like "the child produced nothing" is a defect; "this run recorded no result" is a fact.
- The read must be bounded and must not load an unbounded row into the model's context.

### Frontend or operator surface

None in this milestone. The TUI's run inspector and the delegated-run projection are unchanged; C003 owns the transcript surface. The only visible change is that the orchestrator — and therefore the transcript the user reads in the parent session — may show a child-completion notice it previously did not receive.

### Security and authorization

- The subscription inherits the parent's session, turn, workspace and owner principal. It grants no authority beyond delivering a notice.
- `NotifyOnly` spends no tokens and mints no turn.
- The notice content is a terminal summary of a delegated run the parent already owns. It must not contain credentials, environment values, raw provider reasoning, or unbounded tool output. The delegated run's recorded output is already bounded by the store; the notice carries a **bounded excerpt**, not the whole result.
- A notification must not become a covert channel: the notice text is a function of the delegated run's recorded outcome and the parent's own identifiers.

### Documentation and static guards

- `architecture/agent.md` — what the orchestrator learns about a delegated run and when.
- `architecture/jobs.md` and `architecture/scheduler.md` — that delegated jobs are notification subscribers, with `NotifyOnly`.
- `architecture/tui.md` and `docs/tui.md` — that the delegated tab remains a run inspector in this milestone, and that the parent's transcript may now show a child-completion notice.
- `src/tool/task.rs`'s own description (`src/tool/task.rs:1268`) — it already promises "prefer wait or push notifications over polling get/status"; after this milestone that promise is true for the durable path, and the copy must say which path it holds for.

**Static guard (required).** A new `scripts/check_subagent_notification_wiring.py` with a `--self-test`:

- fails when a delegated-job submission path exists in `src/tool/task.rs` without a matching `subscribe_with_options` call in the same function;
- fails when the disposition passed to that call is not `NotifyOnly`;
- fails when the `kind` token is not the stable agreed token;
- `--self-test` injects each violation into a synthetic tree and proves the guard rejects it, and injects a clean tree and proves it passes.

Wire it into **both** `scripts/verify.sh`'s `run_guards()` and `.github/workflows/ci.yml`.

## 7. Ordered work packages

### Work package A — Establish which delegated path is live

Intent: before changing anything, know which path production takes.

Required changes: none (investigation).

Acceptance evidence: a written finding in the closure record naming the production path, with the wiring that proves it (for example `src/core/daemon_construct.rs:555` and the `TaskTool` construction site), and an explicit statement of what the legacy path guarantees.

### Work package B — Wire the notification service into the `task` tool

Intent: the `task` tool can subscribe.

Required changes: add a `JobNotificationService` handle to the tool's options in the same shape `src/tool/test.rs` uses; wire it from the daemon where the `task` tool is constructed; leave it `None` where a harness or standalone caller has no service, and treat `None` as "skip the subscription", not as an error.

Acceptance evidence: a test that constructs the tool with a service and observes the subscription row; a test that constructs it without one and observes a successful spawn and a logged skip.

### Work package C — Subscribe the delegated job

Intent: the durable notice row exists for every delegated run.

Required changes: per §6.1 — subscribe after submit, before reporting acceptance; `NotifyOnly`; the parent's session, turn, workspace and owner principal; the submission key as the invocation key; skip on an absent parent session; log-and-continue on a subscription failure; fail the spawn on an owner conflict.

Acceptance evidence: the subscription row exists with the expected fields; the disposition is `NotifyOnly`; the ordering is proven by a test that observes no tool success before the subscription exists.

### Work package D — Make the notice identify the run and carry a real summary

Intent: the orchestrator can act on the notice without guessing.

Required changes: per §6 "Protocol and DTOs" — the delegated notice names the run id, and its summary is the delegated run's recorded outcome, bounded, and honest when the outcome is unavailable.

Acceptance evidence: a test that reads the injected `Message::System` and asserts it identifies the run; a test that a run with no recorded output produces a notice that says so rather than reading as an empty success.

### Work package E — Make the child's recorded result readable

Intent: the orchestrator gets content, not a pointer.

Required changes: per §6.3.

Acceptance evidence: a test that `task action=get` returns the recorded result text within the declared bound, that the `result_ref` line is still present, and that a run with no result produces the plain statement.

### Work package F — Make the in-band push non-authoritative in words

Intent: no future reader mistakes the push for the guarantee.

Required changes: per §6.2 — the code and the drop log state that the durable notice is authoritative and the push is a fast path.

Acceptance evidence: the closure record quotes the updated comment and the updated log line, and a reviewer confirms the push is still delivered to a live turn.

### Work package G — Docs, guard and closure

Intent: the repository states the truth and cannot drift back.

Required changes: the documentation updates in §6; the guard with its `--self-test`, wired into `scripts/verify.sh` and `.github/workflows/ci.yml`.

Acceptance evidence: `bash scripts/verify.sh quick` exits 0; the guard's `--self-test` proves non-vacuity; the guard fails against the pre-change tree (armed evidence).

## 8. Failure, cancellation, restart, and contention semantics

- **The daemon restarts between submit and subscribe.** The delegated job exists and the subscription does not. The retry path repairs it: the same invocation key returns the same job (`create_or_get` / `SubmissionKey` idempotency) and completes the subscription. The milestone must prove this ordering property, not assume it.
- **The daemon restarts after the child terminates but before the notice is injected.** The notice row is durable; `inject_recoverable_job_notifications` picks it up at the next turn boundary. This is the case the current in-band push cannot cover, and it is the milestone's central claim.
- **The parent's turn ends before the child terminates.** The notice row is durable and is delivered at the next turn boundary. The in-band push has no live target and drops. The milestone must prove the durable path covers this.
- **The parent's follow-up channel is full.** The in-band push drops (warn-only); the durable row is unaffected. This is the second case the push cannot cover.
- **The delegated run is cancelled.** The notice records the cancelled status and the orchestrator sees it. `NotifyOnly` never resumes a cancelled run (`src/agent/job_notification_recovery.rs:93-95`).
- **Two delegated runs terminate concurrently.** Each has its own job id, its own subscription and its own notice row; exactly one row per logical terminal. The milestone must prove no notice is duplicated and none is lost.
- **The same delegated spawn is retried.** The subscription is idempotent for the same `(session_id, invocation_key)`; a conflicting owner is a spawn failure, never a silent re-target.
- **The subscription row outlives the delegated run.** A stale subscription must not cause a second notice for a later job that reuses the id. The job id is unique per delegated run; the milestone must state the retention behaviour.
- **The notification service is unavailable.** The spawn proceeds; the skip is logged; the orchestrator falls back to polling. No delegated work is lost because of a bookkeeping service.

## 9. Compatibility and migration

- **No migration.** The tables exist and serve other job kinds.
- **No protocol change.** No DTO is added, changed or removed. A client that does not know about delegated notices is unaffected; the notice is injected into the **parent session's** transcript, which every client already renders.
- **No behaviour change for `bash`, `test` or `ci_watch`.** They keep their own subscriptions and dispositions.
- **The legacy delegated path is unchanged** and keeps its poll-only behaviour. The milestone must state this in the documentation rather than letting the docs imply a guarantee the legacy path does not have.
- **The delegated run's `result_ref` is retained**, so nothing that reads it breaks. The tool's `get` output gains a field; it does not lose one.

## 10. Required tests

### 10.1 Focused unit tests — `src/tool/task.rs`, `src/scheduler/job_notifications.rs`

- The subscription is created with the agreed `kind`, the parent's session and turn, the submission key, `NotifyOnly`, the parent's workspace and the owner principal.
- An absent parent session skips the subscription, logs, and the spawn still succeeds.
- A conflicting owner on the same invocation key fails the spawn.
- A subscription failure does not fail the spawn and is logged at `warn`.
- The notice content for a delegated kind identifies the run id.
- A delegated run with no recorded outcome produces a notice that says so.

### 10.2 Integration tests — `tests/agent_loop/`, `tests/scheduler_jobs/`, `tests/core_daemon/`

- **The central case.** A delegated run terminates; its notice row exists; the next parent turn's boundary injects it as a `Message::System` that identifies the run.
- **The parent-turn-ended case.** The parent's turn ends before the child terminates; the next turn still receives the notice. This test must **fail against the pre-change tree**.
- **The dropped-push case.** The parent's follow-up channel is full; the in-band push is dropped; the durable notice is still delivered.
- **The restart case.** The daemon restarts between submit and subscribe; a retry with the same invocation key completes the subscription, and exactly one notice exists afterwards.
- **Concurrency.** Two delegated runs terminate concurrently; exactly one notice per run, none lost.
- **No duplicate notice.** The same delegated run's terminal is recorded once; repeated injection passes do not duplicate the message (`src/agent/job_notification_recovery.rs:135-153`).

### 10.3 Regression tests — `tests/agent_loop/`

- `task action=get` returns the child's recorded result within the declared bound, and still returns the `result_ref` line.
- `task action=get` on a run with no recorded result states that plainly.
- The delegated job is still `RetryPolicy::no_retry()` and `IdempotencyClass::NonIdempotent`.
- The delegated run's tool policy, path set, sandbox and worktree ceiling are unchanged by the subscription.

### 10.4 Armed evidence

The new tests must be shown to **fail against the pre-change tree** and pass after. A test that passes at the baseline is not evidence for this milestone; record the failure output in the closure record.

## 11. Required verification commands

```bash
# narrow first
cargo test -p codegg --lib -- job_notification
cargo test -p codegg --lib -- tool::task
cargo test --test agent_loop -- <member>
cargo test --test scheduler_jobs -- <member>
cargo test --test core_daemon -- <member>

# static guard, with its non-vacuity proof
python3 scripts/check_subagent_notification_wiring.py
python3 scripts/check_subagent_notification_wiring.py --self-test

# the guards this change can plausibly trip
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
bash scripts/check-core-boundary.sh

# formatting and linting
cargo fmt --all
CARGO_BUILD_JOBS=8 cargo clippy --workspace --all-targets --locked -- -D warnings

# the canonical gate
bash scripts/verify.sh quick
```

`bash scripts/verify.sh quick` is the canonical gate and must exit 0. Never run `cargo test --workspace`, a bare `cargo test`, or anything with `--all-features`.

## 12. Documentation updates

- `architecture/agent.md` — what the orchestrator learns about a delegated run, when it learns it, and that the durable notice is authoritative.
- `architecture/jobs.md`, `architecture/scheduler.md` — delegated jobs are notification subscribers with `NotifyOnly`; the subscription is not an admission right.
- `architecture/tui.md`, `docs/tui.md` — the delegated tab remains a run inspector in this milestone; the parent's transcript may now carry a child-completion notice.
- `src/tool/task.rs:1268` — the tool's own description must say which path the "push notifications" promise holds for.
- `plans/registry.md` and the addendum's §12 milestone table — updated at closure, not before.

## 13. Acceptance criteria

1. A delegated run's terminal state produces a **durable** notice row owned by the daemon's existing notification store.
2. The notice is delivered to the parent session at the next turn boundary **even when the parent's turn ended before the child terminated**.
3. The orchestrator can obtain the child's recorded result as content, bounded, without needing a second tool call.
4. A delegated run's notice spends no tokens, mints no turn, creates no session, and grants no authority beyond delivery.
5. A subscription failure, an absent parent session, and a full parent follow-up channel each leave the delegated work intact and the situation visible in the daemon log.
6. The in-band push is retained as a fast path and is documented as non-authoritative.
7. The legacy delegated path is unchanged and its weaker guarantee is stated plainly in the documentation.
8. No migration, no protocol change, and no change to `bash`, `test` or `ci_watch`.
9. The static guard fails against the pre-change tree and passes after, with its `--self-test` proving non-vacuity.
10. `bash scripts/verify.sh quick` exits 0 and `cargo clippy --workspace --all-targets --locked -- -D warnings` is clean.

## 14. Stop conditions

The implementing agent must stop and report rather than improvise when:

- the durable notification store turns out to require a schema change to serve a delegated job — a migration in this milestone is out of scope and indicates the design is wrong;
- the delegated job's `JobRecord` does not carry the parent's session and turn in the production wiring, because the exact-owner routing then has no target;
- the delegated job's terminal summary cannot be made to carry the run's recorded outcome without fabricating content — the notice must then say "outcome unavailable", and that limitation must be recorded rather than papered over;
- subscribing would require `ResumeOwner`, a second notice store, a second scheduler, or any authority the parent does not already hold;
- the legacy path turns out to be the live production path — the durable-notice guarantee then does not apply, the scope must be renegotiated rather than silently narrowed;
- a canonical invariant contradicts this plan — in particular, if a delegated run cannot be given a durable notice without changing the delegated run's identity, stop;
- the change would expand into C002's transcript scope or C004's continuation scope.

## 15. Closure evidence required

The closure record at `plans/closure/delegated-run-observability-and-recovery-corrective/001-status.md` must contain:

1. The requirement-to-evidence matrix for §13, item by item, each row naming a **file:line** and a named test.
2. The **armed evidence**: the new tests run against the pre-change tree with their failure output quoted, and against the post-change tree with their pass output.
3. The production-path finding from work package A, with the wiring that proves it.
4. The exact commands run and their real results, labelled local versus CI — no command that was not run.
5. The guard's `--self-test` output and the guard's failure against the pre-change tree.
6. An invariant review against §4, item by item.
7. A failure-and-recovery review against §8, naming each case and how it was exercised.
8. Any deviation from this plan, with its reason, recorded as a deviation — not as a silent change.
9. The registry updates and the **unblock audit**: whether C002, C003 or C004 became eligible. On this milestone's own evidence, they do not — ADR-0051 is still unaccepted — and the audit must say so explicitly.
10. The closure's status: `closed`, or `conditionally closed` with the exact outstanding evidence named.

## 16. Handoff notes

- **Repository layout.** This work lives in `src/tool/task.rs`, `src/agent/run_control.rs`, `src/scheduler/job_notifications.rs`, `src/agent/follow_up.rs` and `src/agent/job_notification_recovery.rs`, plus the daemon's tool-construction site. Read `src/tool/test.rs:430-478` before writing — it is the model for the subscription.
- **Do not** add a migration, a table, a protocol field, or a second notice mechanism. If one seems necessary, §14 applies.
- **Do not** widen the delegated run's tool, path, sandbox or worktree ceiling. The subscription is a delivery right, not an execution right.
- **Do not** use `ResumeOwner`. It is not authorized for delegated runs.
- **Test conventions.** Root integration tests are **families**: `cargo test --test <family> -- <member>`. `cargo test --test agent_loop` alone runs the whole family. `tests/agent_loop/subagent.inc` is the delegated-run member file; add members there rather than creating a new executable.
- **New `#[tokio::test]`s** default to `current_thread`; use `flavor = "multi_thread", worker_threads = 2` only for real concurrency or subprocesses.
- **Storage tests** use `isolated_pool()`; migrations run inside it, so do not add an extra `migrate()`.
- **Assertion quality.** Every new test body must assert a checkable property or delegate to a named helper that does, or be recorded in `docs/test-assertion-quality.toml` with the reason it asserts nothing. `scripts/check_test_assertion_quality.py` and `scripts/check_test_duplication.py` both run in `verify.sh quick`.
- **Scan Rust with `scripts/rust_test_scan.py`**, never with a regex — a regex scan misreads lifetimes as character literals and undercounts `tests/` by 76%.
- **Budget.** `CARGO_BUILD_JOBS=3` while iterating; 8 for the final workspace clippy. The workspace `cargo check` inside `verify.sh quick` takes about eleven minutes on this machine — that is normal, not a hang.
- **Comment budget.** `scripts/check_comment_budget.py` is a ratchet: re-bank **after** `cargo fmt --all`, and the bank diff must stay confined to the files this milestone touches plus globals.
- **Guard wiring.** The new guard must appear in **both** `scripts/verify.sh`'s `run_guards()` and `.github/workflows/ci.yml`. `verify.sh quick` runs 63 guards and CI runs 49; CI is a strict subset, so a green CI run does not imply a green `quick`.
- **Preserve the user's changes.** The working tree must be clean before starting; do not revert or rewrite unrelated files.
