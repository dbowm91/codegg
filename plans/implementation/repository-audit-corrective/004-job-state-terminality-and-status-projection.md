# Repository Audit Corrective Milestone C004 — Job-State Terminality and Status Projection

Status: closed — closure record at `plans/closure/repository-audit-corrective/004-status.md`

Repository baseline: `3b29c5b88` (`origin/main`)
Audit evidence was gathered at `04536eeeb`; every file this workstream cites is
byte-identical at that commit and at the baseline above, so the findings carry
forward unchanged.

Source roadmap: `plans/subsystems/repository-audit-corrective-roadmap.md#milestone-c004--job-state-terminality-and-status-projection`

Long-term requirements: `plans/000-long-term-specification.md`, `plans/002-long-term-roadmap.md`, `architecture/core.md`

Applicable ADRs: none required. The domain's terminality contract already exists and already decides the disputed question, so this milestone aligns projections with an existing authority rather than defining one.

Primary class: **capability**

## 1. Objective

A job state has exactly one terminality classification, every projection of that state agrees with it, and the model-facing status distinguishes the states it currently flattens. Where a projection answers a *different* question — is this job evidence of success? — that difference is stated in the code rather than implied by a copy of the terminality match.

## 2. Why this milestone is ready

It has no hard or interface dependency, and it owns one defect class in one ownership boundary: the projections of `JobState` outside the domain type that defines it.

It is registered as ready because the authority is already settled. `JobState::is_terminal()` already exists in the domain crate and already answers the disputed question unambiguously. No new architectural decision is required, so no ADR is required.

## 3. Current implementation evidence

### The domain already decides

`crates/codegg-core/src/jobs/mod.rs` defines the authority:

```rust
pub fn is_terminal(&self) -> bool {
    matches!(
        self,
        JobState::Completed
            | JobState::Failed
            | JobState::Cancelled
            | JobState::TimedOut
            | JobState::Expired
    )
}
```

`Interrupted` is **not** terminal. `Scheduled`, `Queued`, `Running` and `Blocked` are not terminal.

### The projections, and how each one answers

| Site | Question it answers | `Interrupted` | Agrees with the domain? |
|---|---|---|---|
| `JobState::is_terminal()` — `crates/codegg-core/src/jobs/mod.rs` | is it finished? | not terminal | **the authority** |
| `is_final_job_state` — `src/scheduler/job_notifications.rs` | is it finished? | not final | yes |
| `record_terminal` — `src/scheduler/job_notifications.rs` | is it finished? | returns `None` — no terminal notice | yes |
| `host_status` — `src/goal_verification.rs` | is this **evidence of success**? | `Failed` | **different question** — see below |
| `host_status` — `src/goal_continuation.rs` | is this **evidence of success**? | `Failed` | same |
| `host_status` — `src/work_plan_evidence.rs` | is this **evidence of success**? | `Failed` | same |
| `terminal_status_str` — `src/scheduler/job_notifications.rs` | what is the status string? | `"interrupted"` | name says "terminal", value is not |
| `terminal_jobs` query — `src/core/daemon_refresh.rs` | which jobs are finished? | **included** | **no — contradicts the domain** |
| live-job query — `src/core/scratch_sweep.rs` | is any job still live? | **included** | yes |
| live-job query — `src/core/daemon_sessions.rs` | is any job still live? | **included** | yes |

### The one genuine contradiction, and its consequence

`src/core/daemon_refresh.rs` builds a query whose local binding is named `terminal_jobs` and whose state list includes `JobState::Interrupted`. That is a terminality claim, and it contradicts the domain.

The consequence is not merely inconsistent naming. `src/core/scratch_sweep.rs` decides whether a session may have its scratch reclaimed by first requiring that every run be terminal, and then by querying for a job in `{Scheduled, Queued, Running, Interrupted, Blocked}` with `limit: Some(1)`. Because an interrupted job is in that set, a session holding an interrupted job is treated as **holding a live job**, and the sweep declines to reclaim the session's scratch. The same set appears in `src/core/daemon_sessions.rs`.

So the divergence is visible as a resource effect: **a session with an interrupted job does not have its scratch reclaimed.** The sweep's reading is the conservative one, so this is a leak rather than a data-loss defect, and the fix direction is to reconcile the two sites against the domain rather than to relax the sweep.

### Three copies of a question that is not the terminality question

The three evidence assemblers each carry their own copy of the same match. Each maps `Interrupted` to `Failed`. That verdict is **correct for the question they are answering** — "does this job count as evidence that the work succeeded?" — and a unit test in `src/goal_verification.rs` names the intent precisely: `terminal_job_states_are_fail_closed`, asserting that `Interrupted` yields `Failed`. Fail-closed evidence is the right behaviour, and it does **not** contradict the domain's terminality: "not finished" and "not evidence of success" are compatible, and in fact consistent.

The defect is that the three copies are written in the shape of a terminality match, in functions named `host_status`, so that a reader sees "terminal states" and reasonably concludes they encode the domain's terminality — while the domain says `Interrupted` is not terminal. A future maintainer who reads the domain and the evidence assembler together sees a contradiction where none exists, and may "fix" either one.

The same three copies are a **maintenance hazard of their own**: three hand-written matches over the same enum will drift the moment a state is added.

### The model-facing status flattens four distinct states

`terminal_status_str` maps `Scheduled`, `Queued`, `Running` and `Blocked` all to `"running"`, and maps `Interrupted` to `"interrupted"`. This is the string the model-facing job tool reports.

The consequence is concrete: a user asking "why has my job not started?" receives "running" for a job that is queued, blocked, or scheduled, and receives no indication that a blocked job is blocked. `Blocked` in particular is a state the scheduler reaches deliberately, and flattening it into "running" hides the reason.

The function's **name** is also wrong for what it does: it is a status projection, not a terminality predicate, and `Interrupted` — a non-terminal state — receives a value from it.

## 4. Invariants that must not regress

- **The domain's terminality is unchanged.** `JobState::is_terminal()` keeps its current membership. This milestone does not redefine terminality; it aligns projections with it.
- **The evidence path stays fail-closed.** A job that did not complete is never evidence of success. `Interrupted`, `Failed`, `Cancelled`, `TimedOut` and `Expired` must continue to fail closed, and `Completed` must continue to pass.
- **The scheduler's terminal notice behaviour is unchanged.** A job in `Interrupted` continues to produce **no** terminal notice, because an eligible attempt may still be requeued.
- **The model-facing status remains additive.** A consumer that already received `"running"` continues to receive a string it can act on; the change distinguishes states, and does not remove a value or change a spelling that a consumer depends on. The implementer must check the consuming surface before renaming a status string, and preserve any spelling a consumer parses.
- **The scratch sweep stays conservative** where the domain's answer is ambiguous. The fix reconciles the two sites against the domain; it must not make the sweep reclaim scratch it is not certain is reclaimable.
- **The projection's retention and limits are unchanged.** No query's `limit` changes, and no query's scope changes.

## 5. Scope

**In scope**

- The terminal-jobs query in `src/core/daemon_refresh.rs`, reconciled with the domain.
- The three copies of the evidence-status match, consolidated into one, with the question they answer made explicit in the code.
- The model-facing status projection, so that the distinct non-terminal states are distinguishable and the function is named for what it does.
- A property test over the job-state enum asserting that every terminality claim in the tree agrees with `JobState::is_terminal()`.

**Out of scope**

- Redefining `JobState::is_terminal()`, adding a state to the enum, or changing the domain's contract.
- Changing the scheduler's requeue behaviour for an interrupted attempt.
- Changing the evidence verdict for any state. The fail-closed behaviour is preserved; only its duplication and its naming change.
- The scheduler's own status enums — `ExecutorStatus::Interrupted` and `AgentRunTerminalOutcome::Interrupted` — which are their own types with their own contracts and are not projections of the domain's terminality.
- The dead routing abstraction in `src/agent/router.rs`; that is C005's.

## 6. Required production changes

### Core

- The terminal-jobs query in `src/core/daemon_refresh.rs` must select exactly the states the domain calls terminal, and must not hand-maintain the list. The implementer must decide whether the store can filter by terminality directly or whether the list is derived from the domain's predicate at the call site — the latter is acceptable if it cannot drift.
- The three evidence-status matches must become **one** function with one authority, and its name and doc must state the question it answers: whether the job counts as evidence of success, not whether it is finished.
- The model-facing status projection must distinguish `Scheduled`, `Queued`, `Running` and `Blocked`. The implementer must choose the spellings, and must check the consuming surface — the model-facing job tool — for anything that parses the current `"running"` value. A spelling that a consumer depends on is preserved, and the new distinctions are **added** rather than substituted, unless the consuming surface is changed in the same change and the change is recorded.

### Storage

None. No stored value changes; the queries change which rows they select, not what is stored.

### Protocol

- The model-facing job status is a protocol-visible string. The change is additive: the four non-terminal states become distinguishable. The implementer must state the exact before-and-after value set and confirm that no consumer parses the old flattened value in a way that breaks.

### Runtime

None.

### Frontend

- The TUI's job presentation may show the distinct states. This is a consequence of the projection change, not a separate deliverable, and the implementer must confirm the TUI does not introduce a second status map that reintroduces the flattening.

### Security

No change to an authorization boundary. One consequence must be checked rather than assumed: the scratch-sweep reconciliation changes **which sessions are eligible for scratch reclamation**. The implementer must confirm the reclaimed set is the one the domain's terminality implies, and must not widen reclamation to a session that still holds a live job.

### Docs

- `architecture/core.md` must state that `JobState::is_terminal()` is the single terminality authority, and that the evidence-status projection answers a different question.

## 7. Ordered work packages

### WP1 — Establish the authority and the inventory

Intent: fix the single source before changing anything.

Required changes: confirm `JobState::is_terminal()` is the intended authority by reading its call sites and the domain's own documentation. Then enumerate every place in the tree that makes a terminality claim about a job — every match over job states, every query whose state list is a terminality claim, and every predicate named for terminality. The result is a table: site, the question it asks, and whether it agrees with the domain.

The implementer must not start from a list carried in from the audit. The divergence is wider than the terminality question — the `Interrupted` handling spans at least eleven sites — and the table must be derived from the code.

Acceptance evidence: the table covers every site found by the enumeration, and each entry states the question the site answers.

### WP2 — Reconcile the terminal-jobs query

Intent: remove the one genuine contradiction.

Required changes: the query in `src/core/daemon_refresh.rs` selects the domain's terminal states. The implementer must then trace the scratch-sweep consequence: with the query corrected, determine whether the scratch sweep's separate live-job query still needs `Interrupted` in its set, and what the correct reclamation behaviour is for a session whose job was interrupted.

The sweep's own reading is conservative and is the safe one. The resolution must be that an interrupted job is **not live** for reclamation purposes if the domain says it is not terminal — but the implementer must check whether the interrupted attempt can still be requeued, because a job that can still run must not have its scratch reclaimed. **If the requeue window means an interrupted job can still become live, the sweep's set is right and the daemon refresh's is wrong** — which is the direction this milestone expects, but the implementer must establish it from the requeue path rather than assume it.

Acceptance evidence: both sites agree with the domain, and the requeue path has been read and cited.

### WP3 — Consolidate the evidence-status projection

Intent: one function, and a name that says which question it answers.

Required changes: replace the three copies with one shared function. Its name must distinguish "evidence of success" from "finished". The fail-closed verdicts are unchanged: `Completed` passes; every other terminal state, and `Interrupted`, fail closed; the non-terminal states are in progress.

The implementer must check where the three sites live — they are in different modules — and place the shared function where all three can reach it without creating a dependency inversion. The domain crate is the natural home if the projection is domain-level; if it is TUI- or daemon-level, it must not be forced into the domain crate, which the repository's core-boundary guard polices.

Acceptance evidence: one function, three call sites, unchanged verdicts, and the existing fail-closed test still passes unchanged.

### WP4 — Distinguish the model-facing status

Intent: stop flattening four distinct states into one.

Required changes: the projection returns a distinct value per state. The implementer must inspect the model-facing job tool and everything that consumes its output, and must preserve any value a consumer parses.

Acceptance evidence: the before-and-after value set is recorded, and every consuming surface is named and checked.

### WP5 — Add the property test and the guard

Intent: make the divergence unrepeatable.

Required changes:

- a property test over the job-state enum asserting that every terminality claim agrees with `JobState::is_terminal()`;
- a guard that fails when a hand-written job-state terminality list appears outside the domain, and when a new `JobState` variant is added without the projections being updated. The guard must use `scripts/rust_test_scan.py` for any Rust parsing, must prove it is not vacuous by injection, and must be wired into `scripts/verify.sh quick` and CI.

Acceptance evidence: the property test and the guard's `--self-test` pass; the guard fails on an injected list.

### WP6 — Record the authority

Intent: prevent the next reader from seeing a contradiction that does not exist.

Required changes: `architecture/core.md` gains a short subsection stating that `JobState::is_terminal()` is the terminality authority, that the evidence projection answers a different question, and that the model-facing status is a distinct projection. It links to the guard and does not restate the guard's implementation.

Acceptance evidence: the subsection exists and is linked from the guard's docstring.

## 8. Failure, cancellation, restart, contention semantics

- **Failure.** A job that did not complete continues to fail closed as evidence. The implementer must not weaken the evidence verdict while reconciling terminality.
- **Cancellation.** A cancelled job remains terminal and remains a failed-evidence state. A job cancelled while `Blocked` or `Queued` keeps its existing transition behaviour.
- **Restart.** A daemon restart re-derives job state from the store. The reconciliation changes which rows the refresh query selects, so the implementer must confirm that a restart cannot lose a projection it previously produced — in particular, a job that was previously projected as terminal because of the inclusive query must still appear somewhere after the fix, or its absence must be deliberate and recorded.
- **Contention.** The queries are store reads on existing paths. The reconciliation must not add a scan, and must not change a query's `limit` or scope, since the projection's bounds are a contract.

## 9. Compatibility and migration

- **Protocol-visible:** the model-facing job status gains distinguishable values for `Scheduled`, `Queued`, `Running` and `Blocked`. This is additive. If any consumer parses the current flattened `"running"` value, that consumer is either updated in the same change or the old value is preserved for it — the implementer must determine which, and the closure records the determination.
- **Durable state:** unchanged.
- **No migration** of stored rows.

## 10. Required tests

**Focused**

- The property test over the job-state enum.
- One test per distinct non-terminal state, asserting the model-facing status distinguishes it.
- The existing fail-closed evidence test, asserting it still passes **unchanged** — the point of WP3 is that the verdicts do not move.
- A test that the terminal-jobs query selects exactly the domain's terminal states.

**Integration**

- A test that a session whose job is interrupted has its scratch reclaimed, or a test asserting the chosen behaviour explicitly if the implementer's requeue analysis concludes the opposite. **The behaviour must be decided and pinned, not left to the divergence.**

**Security**

- A test that the scratch sweep does not reclaim a session that still holds a live job. The reconciliation must not widen reclamation.

**Migration**

- None.

**Placement.** The `scheduler_jobs` family is the right home for the scheduler-side coverage, and `core_daemon` for the daemon-side scratch and projection behaviour. New members are `tests/<family>/<name>.inc`, bound in `tests/<family>/mod.rs` with `#[path = "<name>.inc"] pub mod <name>;`. The family targets are already declared in the root `Cargo.toml`; a new family would need that declaration and is not expected here.

**Vacuity.** The property test must be shown failing against the baseline for the divergent site, and the scratch-reclamation test must be shown failing if the baseline behaviour is the one being changed.

## 11. Required verification commands

```bash
# Scoped: quick's guards plus only the tests this change can affect.
scripts/verify.sh scoped HEAD~1

# The affected families.
cargo test --test scheduler_jobs
cargo test --test core_daemon

# The new guard, including its non-vacuity proof.
python3 scripts/check_job_state_terminality_agreement.py --self-test

# Rust-aware test inventory (ground truth; never regex-scan Rust).
python3 scripts/rust_test_scan.py --validate

# Full workspace, before the closure record is written.
bash scripts/verify.sh full
```

`scripts/verify.sh full` runs `cargo clippy --workspace --all-targets --locked -- -D warnings`. On macOS this currently fails for reasons unrelated to this milestone (C005 owns that asymmetry); the implementer must record the failure set before and after and state that the change added and removed no lint finding. Linux must stay green.

## 12. Documentation updates

- `architecture/core.md`: the terminality authority, the evidence projection's distinct question, and the model-facing status projection.
- `docs/`: no user-facing change is required. The status distinction is visible in the model-facing tool output and, if the TUI shows it, in the job presentation.

## 13. Acceptance criteria

Externally observable:

1. A job in `Scheduled`, `Queued`, `Running` or `Blocked` is reported with a status that distinguishes it from the other three.
2. A job in `Interrupted` is not reported as terminal by any surface, and is not reported as failed evidence unless it genuinely failed.
3. The terminal-jobs query and the live-job query agree with `JobState::is_terminal()`.
4. A session whose only job was interrupted has its scratch reclaimed, or the opposite behaviour is explicitly decided, cited and pinned by a test.
5. The evidence verdicts are unchanged: `Completed` passes, and every non-completing state fails closed.
6. The property test and the new guard's `--self-test` pass, and the guard runs in `verify.sh quick`.

## 14. Stop conditions

The implementer must report rather than improvise if any of the following holds.

- The requeue analysis shows that an interrupted job can still become live, and therefore the sweep's set is right and the daemon refresh's is wrong. That is the **expected** direction, but if the analysis shows the opposite — that an interrupted job is still live for reclamation purposes — the reconciliation needs a decision this milestone does not have authority to make. Record it.
- A consumer of the model-facing job status parses the current flattened `"running"` value in a way that breaks when the states are distinguished. Stop: the protocol change and the consumer change must be decided together.
- The evidence projection cannot be shared without violating the repository's core-boundary rule — that is, the domain crate must stay free of UI, server, plugin and auth concerns, and the projection is not a domain concern. Stop and place it correctly rather than forcing it.
- The consolidation would require changing an evidence verdict. Stop: the verdicts are not this milestone's to change.
- The property test reveals a terminality claim that this milestone's scope does not cover, in a subsystem it does not own. Stop and record it as a separate finding with an owner.

## 15. Closure evidence required

- The WP1 inventory, covering every job-state terminality claim in the tree, and each entry's answer to "which question does this site ask".
- The requeue-path analysis, cited, and the resulting reconciliation of the two query sites.
- The before-and-after model-facing status value set, and the check of every consuming surface.
- The demonstration that the evidence verdicts are unchanged.
- The property test's result, and the guard's `--self-test` output and its presence in `verify.sh quick`.
- The `verify.sh scoped` and `verify.sh full` results, labelled local versus CI, with the macOS lint set stated before and after.
- The registry update, and the unblock audit of the registry's Blocked work section.

## 16. Handoff notes

- **The domain already decides.** `JobState::is_terminal()` exists, it is unambiguous, and it excludes `Interrupted`. This milestone aligns projections with an existing authority; it does not define one. No ADR is needed, and the roadmap's decision point D2 is discharged by the domain's existing contract rather than by a new record.
- **The evidence verdict is not wrong.** `Interrupted → Failed` as *evidence* is correct and is deliberately fail-closed, with a test that names the intent. Do not "fix" it while aligning terminality. The fix is that three copies become one, and that the name says which question is being answered.
- The one **genuine** contradiction is in `src/core/daemon_refresh.rs`, whose query binding is named `terminal_jobs` and includes `Interrupted`. Its visible consequence is in the scratch sweep, which declines to reclaim a session holding an interrupted job. That is a leak, not a data-loss defect, and the conservative reading is the safe one to preserve while the reconciliation is established.
- Do not start from the audit's site list. The `Interrupted` handling spans at least eleven sites and the classification of each depends on the question it asks.
- This milestone closes no other milestone. C005 depends on it.
