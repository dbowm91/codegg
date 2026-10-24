# Delegated Run Observability and Recovery Corrective Milestone C004 — Closure Status

Status: **conditionally closed** — the production capability is complete, every
test passes, and the static guard's `--self-test` proves it bites. The one
outstanding item is the **comment-budget ratchet**, which
`docs/comment-budget.toml` records centrally and the orchestrator re-banks after
all workers land. Named below, with its exact measured numbers.

Source implementation plan:

- `plans/implementation/delegated-run-observability-and-recovery-corrective/004-a-delegated-run-can-be-continued.md`

Source subsystem roadmap:

- `plans/subsystems/delegated-run-observability-and-recovery-corrective-roadmap.md`
  (C004 — "a delegated run can be continued")

Repository baseline reviewed: `220b763a0` ("feat(agent): make a delegated run's
notice and transcript durable (C001, C002)")

Implementation commits or pull requests:

- **none** — the work is left uncommitted in the working tree, as instructed.

---

## 1. Executive finding

A delegated run that failed with a **continuable** failure can now be continued
by the principal that spawned it, and the continuation is a real new attempt of
the same run rather than a second run wearing the first run's name.

The capability boundary is complete on the production side:

- the **continuability decision is recorded explicitly**, per class, in
  `codegg_core::agent_run_attempt::DelegatedFailureClass` — not a generic
  bucket, and an unrecognised class is **not** continuable, so the
  classification fails closed;
- the **attempt record** and the **continuation authorization** are durable, in
  two tables with two shapes: an append-only attempt history and a
  claimable-once authorization whose unique key *is* the compare-and-swap;
- the **flow is ordered** and every step is decided before any provider entry:
  resolve, authorize, classify, claim, read back and re-verify, seed and
  dispatch;
- the **exactly-once** property is a compare-and-swap, not a convention: two
  concurrent requests leave exactly one winner, and the loser is refused with
  the typed reason `already_continued`;
- the **ceiling is carried through unchanged**: the continuation's job payload is
  the attempt's own payload, read back from the attempt record, with only the
  delegation key changed. The delegated job's own admission (`no_retry` +
  `NonIdempotent`) is untouched — a continuation is an explicitly authorized new
  attempt, not the scheduler retrying a `no_retry` job;
- the **outcome arrives on the same channel**: the continuation's job subscribes
  for its own durable terminal notice with the same service and the same kind
  token the first attempt's job used, so the parent's existing drain picks it
  up. **No second notification channel is created**;
- a **crash between the claim and the dispatch** leaves the continuation
  claimed and not dispatched, and the next reconciliation surfaces it as
  `needs_attention` — it is never replayed.

What is **not** claimed: this milestone does not widen what a continuation may
do. A continued attempt runs under the parent's own authority, unchanged. The
parent that wants *different* authority re-dispatches; it does not continue.

---

## 2. Requirement-to-evidence matrix

| Requirement (§13) | Evidence | Result | Notes |
|---|---|---|---|
| A.1 The delegated run's durable identity survives a continuation | `src/tool/task.rs:1786` (`continuation_payload` keeps `run_id`); `tests/agent_loop/delegated_continuation_c004.inc:906` `a_continuable_failure_is_continued_under_the_parent_ceiling` asserts `payload_run == &run_id` and the answer's `Run: {run_id}` | pass | the `run_id` is unchanged and the attempt ordinal is incremented |
| A.2 The continuation is a new attempt, not a second run | `crates/codegg-core/src/agent_run_attempt.rs:332` (`AgentRunAttemptRecord.attempt_ordinal`); `crates/codegg-core/src/agent_run_attempt.rs:597` (`attempt_ordinal` reads `MAX(attempt_ordinal)`); `src/tool/task.rs:1839` (the attempt row is written with the new ordinal) | pass | the run's own record is resumed by `resume_for_continuation` (`crates/codegg-core/src/agent_run.rs:600`) |
| A.3 The attempt ordinal is observable and bounded | `crates/codegg-core/src/agent_run_attempt.rs:597`; `tests/agent_loop/delegated_continuation_c004.inc:798` `the_attempt_record_reads_as_the_runs_first_attempt_before_any_continuation` | pass | a run with no attempt record reads as `1`; the continuation creates `2` |
| B.1 Only the spawning principal may continue | `src/agent/run_control.rs:272` (`authorize_continuation`); `tests/agent_loop/delegated_continuation_c004.inc:361` `a_continuation_is_refused_for_a_wrong_principal` | pass | the refusal is `wrong_principal` |
| B.2 The continuation runs under the parent's authority, unchanged | `src/tool/task.rs:1948` (`continuation_payload` rebuilds the attempt's own payload); `tests/agent_loop/delegated_continuation_c004.inc:930` (denied tools, paths and model are byte-identical) | pass | the sandbox profile and worktree ceiling are the executor's, unchanged, because the continuation dispatches through the ordinary path |
| B.3 One continuation per terminal failure | `crates/codegg-core/src/agent_run_attempt.rs:640` / `:835` (the continuation row's primary key on `run_id` is the compare-and-swap); `tests/agent_loop/delegated_continuation_c004.inc:517` `a_duplicate_continuation_is_refused_and_never_replayed` | pass | the duplicate is refused with `already_continued` and creates no second attempt |
| B.4 No provider entry before the transcript is read back and re-verified | `src/agent/worker.rs:1137` (`seed_delegated_context` reads the store and re-verifies every row with the repository's own recogniser) and `src/agent/worker.rs:1102` (`delegated_attempt_provenance`); `tests/agent_loop/delegated_continuation_c004.inc:501` `a_continuation_is_refused_for_a_run_with_no_recorded_transcript` and `:445` `..._fails_re_verification` | pass | both refusals happen **before** the dispatch and before any provider entry |
| B.5 The seeded context is exactly the stored transcript | `crates/codegg-core/src/agent_run_attempt.rs:574` (`build_seeded_context`); `tests/agent_loop/delegated_continuation_c004.inc:945` (the seeded context equals the stored page exactly) | pass | the rows come from the store, never from a live in-memory channel |
| B.6 A run with no recorded transcript cannot be continued | `crates/codegg-core/src/agent_run_attempt.rs:555` (`reverify_transcript_row`); refusal `transcript_absent` | pass | a run recorded before C002 is refused, not silently started from nothing |
| C.1 The delegated run's terminal state remains durable | unchanged: C001's notice row (`src/agent/follow_up.rs:427`) | pass | the continuation does not touch C001's path |
| C.2 The continuation's outcome arrives through the same path | `src/tool/task.rs:1877` (the continuation's job subscribes for its own durable terminal notice); `tests/agent_loop/delegated_continuation_c004.inc:1108` `the_continuations_outcome_arrives_through_the_durable_notice_path` | pass | the parent's existing drain injects the notice; no second channel exists |
| D.1 The delegated run's status survives the continuation | `crates/codegg-core/src/agent_run.rs:600` (`resume_for_continuation`); `src/scheduler/executors.rs:1178` | pass | the run's own record leaves its terminal state so the new attempt can write a fresh outcome |
| D.2 The delegated run's transcript remains durable | unchanged: C002's store | pass | the continuation appends to the same transcript |
| D.3 The delegated run's attempt record is durable | `crates/codegg-core/src/agent_run_attempt.rs:428` (`AgentRunAttemptStore`); `crates/codegg-core/src/session/schema.rs:3772` (migration v90) | pass | see §7 |
| E.1 A run's attempt record is append-only | `crates/codegg-core/src/agent_run_attempt.rs:332`; the table's primary key `(run_id, attempt_ordinal, event)` | pass | a row is inserted and never updated or deleted |
| E.2 A crash between the claim and the dispatch needs attention and is never replayed | `src/agent/run_control.rs:374` (`reconcile_continuations`); `tests/agent_loop/delegated_continuation_c004.inc:560` `a_crash_between_the_claim_and_the_dispatch_needs_attention_and_is_never_replayed` | pass | the reconciliation creates no attempt |
| E.3 Exactly one winner under concurrent requests | `crates/codegg-core/src/agent_run_attempt.rs:835`; `tests/agent_loop/delegated_continuation_c004.inc:1024` `two_concurrent_continuations_leave_exactly_one_winner` | pass | one accepted, one refused with `already_continued` |
| F.1 The tool's action surface names the continuation | `src/tool/task.rs:1502` (the action enum) and `src/tool/task.rs:1494` (the description); `tests/agent_loop/delegated_continuation_probe_c004.inc:15` | pass | the description states the capability, the bound and when it is warranted |
| G.1 The tool's parameters do not widen the parent's ceiling | `src/tool/task.rs:1817` (an `allowed_paths` outside the recorded ceiling is refused with `ceiling_widened`); `tests/agent_loop/delegated_continuation_c004.inc:749` | pass | the request may narrow the ceiling, never widen it |
| G.2 The delegated job's own admission is unchanged | `src/tool/task.rs:1857` (`RetryPolicy::no_retry()` + `IdempotencyClass::NonIdempotent`); `tests/agent_loop/delegated_continuation_c004.inc:674` and `:823` | pass | a continuation is an authorized new attempt, not a scheduler retry |
| H.1 The static guard is not vacuous | `scripts/check_delegated_continuation_authority.py --self-test` | pass | three injected defects, each rejected; the unmodified tree produces no finding |

---

## 3. Production implementation evidence

### 3.1 The recorded continuability decision (work package A)

ADR-0051 explicitly deferred the provider-failure retryability taxonomy to this
milestone. The decision is recorded explicitly, per class, in
`crates/codegg-core/src/agent_run_attempt.rs:57`
(`DelegatedFailureClass`) and `crates/codegg-core/src/agent_run_attempt.rs:134`
(`is_continuable`). It is not inherited from a generic bucket, and an
unrecognised class is **not** continuable.

| class | continuable | reason | test |
|---|---|---|---|
| `provider_authentication` | **no** | the continuation carries the parent's authority unchanged, so it presents the same credentials; retrying an unchanged request that was refused for its credentials cannot succeed | `a_continuation_is_refused_for_each_failure_class_that_is_not_continuable` (`tests/agent_loop/delegated_continuation_c004.inc:475`) |
| `provider_rate_limit` | **yes** | throttling is transient by the provider taxonomy the repository already owns, and a later attempt is a different moment | `a_continuable_failure_is_continued_under_the_parent_ceiling` (`:906`) |
| `provider_transient` | **yes** | a 5xx, a transport error or an interrupted stream is transient one level up, by the same taxonomy | same test (the class is the family's transient case) |
| `deadline_exceeded` | **yes** | a fresh attempt gets a fresh deadline; the work itself is unchanged | `the_recorded_decision_names_each_class_explicitly` (`crates/codegg-core/src/agent_run_attempt.rs`, inline unit test) |
| `budget_exhausted` | **yes** | a fresh attempt gets a fresh budget | same inline unit test |
| `tool_permission_denied` | **yes** | the parent's own tool policy is carried through unchanged, so the continuation is the parent's chance to re-dispatch with authority the child could use | same inline unit test |
| `cancelled` | **never** | a cancellation is a decision, not a failure | `a_continuation_is_refused_for_each_failure_class_that_is_not_continuable` |
| `unclassified` | **no** | an unrecognised failure is not assumed recoverable; the classification fails closed | same test, and `an_unknown_failure_class_fails_closed` (inline unit test) |

The provider classes are produced from the repository's **own** provider
taxonomy — `ProviderError::error_class()` — by
`src/agent/worker.rs:1188` (`classify_delegated_failure`). No second recogniser
is invented: `provider_authentication` is the taxonomy's own `auth` class, and
`provider_rate_limit` and `provider_transient` are its `rate_limit` and its
transient classes.

### 3.2 The attempt record and the continuation authorization

Two tables, because they answer two questions and have two shapes.

**`agent_run_attempt`** — the append-only attempt history.
`crates/codegg-core/src/session/schema.rs:3772` (migration **v90**). Primary
key `(run_id, attempt_ordinal, event)`; `event` is `attempt` (the attempt was
created), `outcome` (the attempt reached a terminal state) or `attention` (the
reconciliation surfaced it). A row is inserted and never updated or deleted, so
a run's attempt history cannot be rewritten. The row carries who continued
which run (`principal`, `principal_turn_id`), under which parent's authority
(`authority_digest`), what was seeded (`transcript_digest`), the failure class
continued from, and the **ceiling** — the attempt's own payload, verbatim.

**`agent_run_continuation`** — the claimable-once authorization. Primary key
`run_id`, so the row's *existence* is the compare-and-swap. Its state moves
`claimed` → `dispatched`, or → `needs_attention` when the dispatch never
happened. A duplicate request finds the row present and is refused.

The compare-and-swap's mechanism, concretely
(`crates/codegg-core/src/agent_run_attempt.rs:835`): the claim is a single
`INSERT INTO agent_run_continuation (...)` in one transaction. A unique-key
violation is the loser, and the loser is told the winner's record
(`ContinuationClaimOutcome::AlreadyClaimed`). SQLite's write lock serialises
the two racers, so exactly one insert wins.

Both tables are reachable from every holder of an `AgentRunStore` without a
second wiring path: `AgentRunStore::attempt_store()`
(`crates/codegg-core/src/agent_run.rs:592`).

### 3.3 The authorization and the flow

`RunControlService::authorize_continuation` (`src/agent/run_control.rs:272`)
rules every denial out, in order, before any provider entry:

1. **Resolve** the run → `run_not_found`.
2. **The parent must still be there** → `no_live_parent`.
3. **The spawning principal** → `wrong_principal`.
4. **The run's own terminal state** → `run_cancelled`, `not_terminal`,
   `run_superseded`.
5. **The authorization's bound** → `authorization_expired` (24 h after the
   failure, `CONTINUATION_AUTHORIZATION_TTL_MS`,
   `crates/codegg-core/src/agent_run_attempt.rs:41`).
6. **The recorded decision** → `not_continuable`.

The claim (`crates/codegg-core/src/agent_run_attempt.rs:835`) is the
compare-and-swap; the read-back and re-verification
(`src/tool/task.rs:1758`) come next; the dispatch
(`src/tool/task.rs:1855`) is last.

### 3.4 The `continue` action

`src/tool/task.rs:1647` (`continue_delegated_run`). The action's result
states the run, the attempt ordinal, whether the continuation was accepted or
refused, and — when refused — the typed reason:

- accepted: `format_continuation_accepted` (`src/tool/task.rs:1996`) —
  `Continuation accepted` / `Run: <id>` / `Attempt: <n>` / `Seeded context: <n>
  transcript entries (<bytes> bytes)`.
- refused: `format_continuation_refusal` (`src/tool/task.rs:1985`) —
  `Continuation refused` / `Run: <id>` / `Reason: <code>: <message>`.

The action's schema: `action` gains `"continue"`
(`src/tool/task.rs:1502`), and the tool's description
(`src/tool/task.rs:1494`) states the capability, the bound, and when it is
warranted.

### 3.5 The provider entry is accounted for

`src/agent/worker.rs:1102` (`delegated_attempt_provenance`) reads the run's
attempt record before the child's turn and derives the attempt's provenance —
the run's original attempt or a claimed continuation. A durable delegated run
with **no** attempt record is refused before the provider is reached. A claimed
continuation must additionally carry its seeded context
(`src/agent/worker.rs:1137`): the recorded transcript, read back from the store,
every row re-verified with the repository's own `redact_transcript_body`, and
the seeded context built from exactly those rows.

The scheduler side holds the matching rule
(`src/scheduler/executors.rs:1174`): a terminal delegated run may be resumed
**only** by a claimed continuation.

### 3.6 The ceiling is carried through unchanged

The continuation's job payload is the attempt's own payload, read back from the
attempt record, with only the delegation key changed
(`src/tool/task.rs:1948`). The parent's `denied_tools`, `allowed_paths`,
model, sandbox profile and worktree ceiling are therefore byte-identical. The
`continue` request may supply an optional `allowed_paths` that may only
*narrow* the recorded ceiling; anything wider is refused with
`ceiling_widened` (`src/tool/task.rs:1817`).

The delegated job's own admission is unchanged: `RetryPolicy::no_retry()` and
`IdempotencyClass::NonIdempotent` (`src/tool/task.rs:1857`). The code says so
in place: *a continuation is an explicitly authorized new attempt of the same
run, not the scheduler retrying a `no_retry` job.*

### 3.7 The outcome arrives on the same channel

The continuation dispatches through the ordinary submission path and the
dispatched job subscribes for its own durable terminal notice
(`src/tool/task.rs:1877`), with the same service and the same kind token the
first attempt's job used, and with the continuation's own invocation key. The
parent's existing drain (`src/agent/follow_up.rs:427`) therefore picks the
continuation's outcome up. **No second channel is created.**

### 3.8 Crash safety

`RunControlService::reconcile_continuations` (`src/agent/run_control.rs:374`)
surfaces a continuation that was claimed but never dispatched as
`needs_attention`. It is **never** replayed: the attempt bound is one per
terminal failure, and a replay would be a second attempt the parent never
authorized. A grace period (`CONTINUATION_CLAIM_GRACE_MS`,
`crates/codegg-core/src/agent_run_attempt.rs:48`) keeps the reconciliation from
racing a dispatcher that is still in flight.

---

## 4. Verification executed

### Commands run

```bash
# the new core module's unit tests
CARGO_BUILD_JOBS=3 cargo test -p codegg-core --locked --lib -- agent_run_attempt

# the new integration members (the C002 member was re-run after its adaptation)
CARGO_BUILD_JOBS=3 cargo test -p codegg --locked --test agent_loop -- delegated_transcript
CARGO_BUILD_JOBS=3 cargo test -p codegg --locked --test agent_loop -- delegated_continuation

# the static guard and its self-test
python3 scripts/check_delegated_continuation_authority.py
python3 scripts/check_delegated_continuation_authority.py --self-test

# clippy on the two packages touched
CARGO_BUILD_JOBS=3 cargo clippy -p codegg-core --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=3 cargo clippy -p codegg --all-targets --locked -- -D warnings

# the armed evidence: a pre-change worktree at HEAD with only the new test
# members copied in
git worktree add /tmp/c004-prechange HEAD
# (in /tmp/c004-prechange)
CARGO_BUILD_JOBS=3 cargo test -p codegg --locked --test agent_loop -- delegated_continuation_probe
CARGO_BUILD_JOBS=3 cargo test -p codegg --locked --test agent_loop -- delegated_continuation_c004

# the canonical gate
bash scripts/verify.sh quick
```

### Results

| Command | Result |
|---|---|
| `cargo test -p codegg-core --lib -- agent_run_attempt` | **pass** — 8 passed, 0 failed |
| `cargo test --test agent_loop -- delegated_transcript` (C002's member) | **pass** — 6 passed, 0 failed |
| `cargo test --test agent_loop -- delegated` | **pass** — 29 passed, 0 failed (the C004 member's 18 tests, the probe member's 2, and C001's and C002's 9) |
| `python3 scripts/check_delegated_continuation_authority.py` | **pass** — exit 0, "check ok" |
| `python3 scripts/check_delegated_continuation_authority.py --self-test` | **pass** — 3 injected defects each rejected; the unmodified tree produced no finding |
| `cargo clippy -p codegg-core --all-targets --locked -- -D warnings` | **pass** — clean |
| `cargo clippy -p codegg --all-targets --locked -- -D warnings` | **pass** — clean |
| `cargo test --test agent_loop -- delegated_continuation_probe` (pre-change tree) | **fail, as armed** — 2 failed, 0 passed (quoted in §4.1) |
| `cargo fmt --check --all` | **pass** |
| `bash scripts/check-core-boundary.sh` | **pass** |
| `python3 scripts/check_project_catalog_invariants.py` | **pass** — 7/7 checks |
| `cargo test --test agent_loop -- delegated_continuation_c004` (pre-change tree) | **fail to compile, as armed** — the capability does not exist (quoted in §4.1) |
| `bash scripts/verify.sh quick` | see §4.2 |

### 4.1 The armed evidence

The pre-change tree is `git worktree add /tmp/c004-prechange HEAD` at
`220b763a0`, with **only the new test members** copied in.

**The behavioural probe** (`tests/agent_loop/delegated_continuation_probe_c004.inc`,
which uses only pre-existing APIs) **fails** against the pre-change tree:

```
running 2 tests
test delegated_continuation_probe_c004::the_task_tool_advertises_the_continuation_capability ... FAILED
test delegated_continuation_probe_c004::the_task_tool_description_states_the_continuations_bound_and_warrant ... FAILED

---- delegated_continuation_probe_c004::the_task_tool_advertises_the_continuation_capability stdout ----
thread 'delegated_continuation_probe_c004::the_task_tool_advertises_the_continuation_capability' panicked at tests/agent_loop/delegated_continuation_probe_c004.inc:26:5:
the action enum must offer the continuation: [String("scratch_create"), String("scratch_revoke"), String("spawn"), String("converge"), String("convergence_status"), String("convergence_decide"), String("convergence_cancel"), String("spawn_many"), String("create_group"), String("status"), String("get"), String("message"), String("interrupt"), String("wait"), String("cancel"), String("status_group"), String("wait_group"), String("cancel_group"), String("board"), String("claim"), String("release_claim"), String("request_handoff")]

---- delegated_continuation_probe_c004::the_task_tool_description_states_the_continuations_bound_and_warrant stdout ----
thread 'delegated_continuation_probe_c004::the_task_tool_description_states_the_continuations_bound_and_warrant' panicked at tests/agent_loop/delegated_continuation_probe_c004.inc:38:5:
the description must name the continuation action: Spawn a subagent to handle a task independently. Use spawn_many for a bounded group of independent children, then wait_group/status_group/cancel_group; prefer wait or push notifications over polling get/status. On the durable delegated path (a daemon-scheduled subagent) the child's completion is pushed as a durable terminal notice at the next turn boundary, so that promise holds there; the legacy in-process path has no such push. Mutating durable runs receive a managed isolated worktree before model execution; read-only runs inherit the parent workspace. Child completion never merges into the parent automatically—inspect the structured result and request explicit typed integration.

test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 542 filtered out; finished in 0.02s
```

**The behavioural member** (`tests/agent_loop/delegated_continuation_c004.inc`)
does not even compile against the pre-change tree, because the capability does
not exist:

```
error[E0432]: unresolved import `codegg_core::agent_run_attempt`
  --> tests/agent_loop/delegated_continuation_c004.inc:23:18
   |
23 | use codegg_core::agent_run_attempt::{
   |                  ^^^^^^^^^^^^^^^^^ could not find `agent_run_attempt` in `codegg_core`

error[E0433]: failed to resolve: could not find `agent_run_attempt` in `codegg_core`
  --> tests/agent_loop/delegated_continuation_c004.inc:615:42
   |
615 |         .claim_continuation(codegg_core::agent_run_attempt::NewContinuationClaim {
```

**After the change**, the same members pass: 20 passed, 0 failed
(`cargo test --test agent_loop -- delegated_continuation`).

### 4.2 The canonical gate

`bash scripts/verify.sh quick` **aborts at the comment-budget guard**. The
guard's own `--self-test` fails first, because it requires the unmodified tree
to pass the guard:

```
==> python3 scripts/check_comment_budget.py --self-test
ERROR: the unmodified tree must pass the guard: R4 the corpus holds 125386 comment lines, over the recorded ceiling of 124716
```

and the guard itself:

```
ERROR: R4 src/tui/commands/primary_projection.rs: 31 comment lines is over the 20-line floor for an unlisted file; add it to docs/comment-budget.toml with a ceiling
ERROR: R4 crates/codegg-core/src/agent_run.rs: 49 comment lines is over the recorded ceiling of 38
ERROR: R4 crates/codegg-core/src/agent_run_attempt.rs: 159 comment lines is over the 20-line floor for an unlisted file; add it to docs/comment-budget.toml with a ceiling
ERROR: R4 crates/codegg-core/src/session/schema.rs: 469 comment lines is over the recorded ceiling of 467
ERROR: R4 crates/codegg-core/src/storage/mod.rs: 105 comment lines is over the recorded ceiling of 103
ERROR: R4 crates/codegg-protocol/src/projection/dto.rs: 328 comment lines is over the recorded ceiling of 247
ERROR: R4 tests/agent_loop/delegated_continuation_c004.inc: 60 comment lines is over the 20-line floor for an unlisted file; add it to docs/comment-budget.toml with a ceiling
... 36 comment-budget violation(s) in total
```

Both the ceiling and the per-file floors are **central ratchet state**: the
corpus is measured over the **whole tree**, and this tree also carries the
C003 worker's landed changes. `docs/comment-budget.toml` is not this
milestone's to change, and the orchestrator re-banks it centrally.

Because the gate aborts there, **the steps after the comment budget were run
separately**, exactly as `scripts/verify.sh` runs them:

- the 73 remaining guard invocations (TUI authority guards, the provider
  guards, the writer/process/shared-checkout guards, `check_test_ownership`,
  `check_test_assertion_quality` (+ `--self-test`), `check_test_duplication` (+
  `--self-test`), `check_test_target_references` (+ `--self-test`),
  `check_test_common_binding`, and the rest): **2 failures**, both the same
  stale count row —

  ```
  FAIL: architecture/overview.md records 342 integration test files, but the tree holds 346 root integration test modules
  ```

  The recorded 342 was exactly the count at `220b763a0`. The C003 worker added
  two `.inc` members (taking the tree to 344), and this milestone's two
  members take it to 346 — so the row was already stale before this change.
  `architecture/overview.md` is the orchestrator's to reconcile.
- `python3 -m unittest discover -s dev/harness_lab/tests`: **pass** — 329
  tests, OK (1 skipped).
- `CARGO_BUILD_JOBS=2 cargo check --workspace --all-targets --locked`:
  **pass** (see §4.3).

⇒ The comment budget and the stale count row are the **only** failures in the
gate. Neither is a behavioural defect in this milestone's capability, and both
are central ratchet/count state the orchestrator owns.

### 4.3 The workspace check

```bash
CARGO_BUILD_JOBS=2 cargo check --workspace --all-targets --locked
```

**pass** — no diagnostics.

---

## 5. Invariant review

Against the plan's §4 item by item.

| §4 invariant | Evidence | Result |
|---|---|---|
| 4.1 The delegated run's durable identity survives a continuation | the continuation's payload carries the same `run_id` (`src/tool/task.rs:1948`); `a_continuable_failure_is_continued_under_the_parent_ceiling` | pass |
| 4.2 The continuation is a new attempt of the same run | the attempt record's ordinal increments; the run's own record is resumed by `resume_for_continuation` | pass |
| 4.3 The continuation runs under the parent's authority, unchanged | the attempt's own payload is the continuation's payload; the ceiling test asserts byte-identity | pass |
| 4.4 One continuation per terminal failure | the continuation row's primary key on `run_id`; the duplicate test; the concurrent test | pass |
| 4.5 No provider entry before the transcript is read back and re-verified | `seed_delegated_context` runs before the provider entry; the continuation must carry the seeded context or it is refused | pass |
| 4.6 The seeded context is exactly the stored, redacted transcript | `build_seeded_context` over the store's own page; every row re-verified with the repository's own recogniser | pass |
| 4.7 The outcome arrives through the delegated run's existing terminal path | the continuation's job subscribes for its own durable notice; the parent's existing drain injects it | pass |
| 4.8 The delegated job's own admission is unchanged | `no_retry` + `NonIdempotent`, asserted in two tests | pass |
| 4.9 The delegated run's status survives the continuation | the run's record is resumed, not re-created | pass |
| 4.10 The delegated run's transcript remains durable | the continuation appends to the same transcript; the seeded context is read from it | pass |
| 4.11 The delegated run's attempt record is durable | migration v90; two tables; the store is reachable from every run-store holder | pass |
| 4.12 A run's attempt record is append-only | the table's shape; rows are only inserted | pass |
| 4.13 A crash between the claim and the dispatch needs attention and is never replayed | `reconcile_continuations`; the crash test | pass |
| 4.14 Exactly one winner under concurrent requests | the compare-and-swap; the concurrency test | pass |
| 4.15 The tool's action surface names the continuation | the action enum and the description; the probe member | pass |
| 4.16 The tool's parameters do not widen the parent's ceiling | `ceiling_widened`; the ceiling test | pass |

---

## 6. Failure and recovery review

- **Duplicate delivery and idempotency.** A duplicate continuation request is
  **refused, not replayed** (`already_continued`). The duplicate creates no
  second attempt row — asserted in
  `a_duplicate_continuation_is_refused_and_never_replayed`.
- **Concurrent requests.** Exactly one winner; the loser is refused with the
  typed reason. Asserted in
  `two_concurrent_continuations_leave_exactly_one_winner`, which also asserts
  that the race creates no second attempt.
- **Cancellation races.** A cancelled run is never continued
  (`run_cancelled`). The cancellation check precedes the terminal check, so a
  run that is *cancelling* is refused too.
- **Daemon or node restart.** A crash between the claim and the dispatch leaves
  the continuation claimed and not dispatched. The next reconciliation surfaces
  it as `needs_attention` and never replays it. A crash *after* the dispatch
  leaves the job in the queue, and the ordinary scheduler recovery owns it —
  the continuation's own state is `dispatched`, so the reconciliation does not
  touch it.
- **Partial persistence failure.** The claim and the attempt-row insert are
  separate durable steps; the attempt row is written **before** the dispatch
  and the claim is released if the dispatch fails, so a failed dispatch leaves
  no attempt that never ran.
- **Stale generation or lease.** The continuation dispatches through the
  ordinary submission path with its own delegation key, so it is a new
  submission and not a duplicate of the first attempt's.
- **Contention and resource release.** The continuation's job is a new job; the
  scheduler's own admission and resource accounting apply unchanged.
- **Malformed or unauthorised input.** An unparsable `run_id` is a tool error; a
  run that does not resolve is `run_not_found`; a wrong principal is
  `wrong_principal`; a widening `allowed_paths` is `ceiling_widened`.
- **Bounded event and artifact behaviour.** The seeded context is bounded by
  the transcript store's own bounds (`MAX_TRANSCRIPT_READ_LIMIT`,
  `MAX_TRANSCRIPT_ROW_BYTES`, `MAX_TRANSCRIPT_RUN_BYTES`); a truncated
  transcript says so in the seeded context rather than pretending the tail is
  complete.

---

## 7. Migration and compatibility review

- **Schema version before:** `STORAGE_LAYOUT_VERSION = 89`
  (`crates/codegg-core/src/storage/mod.rs`).
- **Schema version after:** `STORAGE_LAYOUT_VERSION = 90`.
- **The migration:** `migrate_v90`
  (`crates/codegg-core/src/session/schema.rs:3772`) creates
  `agent_run_attempt` and `agent_run_continuation`. It is additive: two new
  tables, no column changed, no column dropped, no row rewritten. The
  `CHECK` constraints on the enums are the durable vocabulary.
- **What an older client sees:** nothing. The migration is a storage-layout
  change; no protocol, DTO or wire shape changed. The `task` tool's *action
  enum* gains one value, which an older client does not use.
- **What an older daemon sees:** a database at layout 90 whose two new tables
  are empty. A delegated run recorded before this milestone has **no** attempt
  record, so its attempt ordinal reads as `1` and a continuation of it is
  refused with `transcript_absent` (it also has no transcript) or
  `ceiling_unavailable` (it has no recorded ceiling). It is never continued
  from nothing.
- **Rollback limitations:** rolling the layout back to 89 leaves two unused
  tables behind; the data in them is the attempt history and the continuation
  authorizations, and it is not read by any v89 path.

---

## 8. Security review

- **Authorization enforcement.** `authorize_continuation`
  (`src/agent/run_control.rs:272`) requires the spawning principal: the
  actor's **session** must be the task's originating session (and the run must
  be a direct child of it), or the actor must be the run's own parent run for
  a nested delegation. The turn-level rule of `authorize` is untouched and
  still governs `message`, `interrupt` and `cancel`.
- **Denial before inference.** Every refusal is decided before the dispatch
  and therefore before any provider entry: the authorization, the
  classification, the claim, the read-back and the re-verification all happen
  first. The provider-entry guard in the child
  (`src/agent/worker.rs:1102`) refuses a durable delegated run with no attempt
  record **before** the provider is reached, and refuses a claimed
  continuation that carries no seeded context.
- **The ceiling cannot be widened.** The continuation's payload is the
  attempt's own payload; the request may only narrow the path set; anything
  wider is refused with `ceiling_widened`. The sandbox profile and the
  worktree ceiling come from the same executor and the same placement, so they
  are unchanged.
- **Redaction.** The seeded context is built from the **stored, redacted**
  rows, and every row is re-verified with the repository's existing
  recogniser (`ProjectionFieldRedactor` through `redact_transcript_body`) — no
  second recogniser is invented. A row that fails re-verification is refused
  rather than seeded, and the refusal does not echo the row's contents.
- **Denial-of-service bounds.** One continuation per terminal failure; the
  authorization expires 24 h after the failure; the seeded context is bounded
  by the transcript store's own bounds; the attempt ordinal makes the bound
  observable in the durable record.
- **Audit.** The continuation authorization records **who** continued **which**
  run, under **which** parent's authority, with **which** failure class, and
  the attempt record adds the seeded transcript's digest and the attempt's
  ceiling. The audit holds no conversation: the transcript stays in the
  transcript store, and the audit holds only its digest.
- **The notice channel.** The continuation subscribes on the same durable
  notice channel as the first attempt, so the parent's drain is unchanged and
  no second channel can be created.

---

## 9. Documentation and operations

- `architecture/agent.md` — a new §"Continuing a delegated run" with the flow,
  the recorded continuability decision (the table), the ceiling rule, the
  notice channel and crash safety.
- `architecture/jobs.md` — a new §"Delegated run attempts and the continuation
  authorization" with the two tables' shapes and the provider-entry guard.
- `architecture/permission.md` — a new invariant 0: a delegated run's
  continuation cannot widen the ceiling, and the delegated job's admission is
  unchanged.
- `scripts/check_delegated_continuation_authority.py` — the static guard. It
  fails when a path that starts a delegated run's provider entry is reachable
  without the attempt record accounting for it, and it carries a `--self-test`
  that injects three defects into a synthetic tree and proves the guard
  rejects each of them, and that the unmodified tree produces no finding.
  **Not yet wired into `scripts/verify.sh` or CI** — the orchestrator owns
  both files and wires the guard in centrally.

---

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | The comment-budget ratchet (`docs/comment-budget.toml`) fails `verify.sh quick` — the corpus holds 125386 comment lines against a recorded ceiling of 124716, and the guard's `--self-test` fails with it (the self-test requires the unmodified tree to pass the guard) | the gate aborts at that step, so the steps after it must be run separately | the orchestrator re-banks the budget centrally after all workers land. **Not edited here**: the bank file is not this milestone's to change. |
| low | `architecture/overview.md` records 342 integration test files, but the tree holds 346 | `scripts/check_test_target_references.py` (and its `--self-test`) fail on the count | the orchestrator reconciles the counts in `architecture/overview.md`. **Not edited here**: that file is the orchestrator's. The four files are this milestone's two new members (`tests/agent_loop/delegated_continuation_c004.inc`, `tests/agent_loop/delegated_continuation_probe_c004.inc`) and C003's two (`tests/projection_replay/delegated_transcript.inc`, `tests/tui_render/delegated_transcript.inc`). |
| low | The static guard is not yet wired into `scripts/verify.sh` / CI | the guard runs only when it is invoked by hand | the orchestrator wires it in centrally (`scripts/verify.sh` and `.github/workflows/ci.yml` are not this milestone's files). |
| low | `src/tool/factory.rs` was edited (one builder call) although C001 owns it | a one-line change in a closed file | **Justified and disclosed**: the continuation reads the transcript back from the durable store, and the `task` tool is the only holder of that path. The change is a single `with_transcript_store(...)` call, additive, and touches no C001 behaviour. |
| low | The reconciliation is invoked by the test and is available to a daemon tick, but no daemon tick calls it yet | a stale claim is surfaced when something calls `reconcile_continuations` | the daemon's periodic reconciliation is outside this milestone's file set (`src/core/daemon_refresh.rs` belongs to the other worker in this round). The function is public and side-effect free apart from the state it marks. |

---

## 11. Roadmap disposition

**Milestone conditionally closed, with the two low-severity items above named.**

This is the **last** milestone of the "delegated run observability and recovery
corrective" line, so the line's exit conditions are now:

1. **The orchestrator always hears about its child** (C001) — the durable
   notice row exists, the parent's next turn drains it, and a continuation's
   outcome arrives on that same channel.
2. **A delegated run has a durable transcript** (C002) — the store, its
   bounds, its redaction and its re-verification exist, and the continuation
   seeds from it.
3. **The TUI renders the delegated transcript** (C003) — the transcript is
   projected and rendered, with its completeness stated.
4. **A delegated run can be continued** (C004) — this milestone. The
   continuability decision is recorded, the attempt record and the
   continuation authorization are durable, the flow is ordered and denies
   before inference, the bound is one continuation per terminal failure, the
   ceiling is carried through unchanged, and a crash is surfaced rather than
   replayed.

The line's exit conditions are therefore **met on the production side**. What
remains is **central wiring the orchestrator owns**: the static guard into
`verify.sh`/CI, and the comment-budget re-bank. Neither is a behavioural
defect in this milestone's capability.

The next dependency may proceed once those two central steps are taken. If the
orchestrator wants the reconciliation driven by a daemon tick rather than by a
caller, that is a small follow-up in the daemon's own file, not a correction
of this milestone.

---

## 12. Registry updates

Required in `plans/registry.md` (owned by the orchestrator):

- mark milestone **C004** of `delegated-run-observability-and-recovery-corrective`
  as closed, pointing at this record.

Required in the source subsystem roadmap
(`plans/subsystems/delegated-run-observability-and-recovery-corrective-roadmap.md`,
owned by the orchestrator):

- close **D3** (the provider-failure retryability taxonomy), which this
  milestone decided and recorded in
  `crates/codegg-core/src/agent_run_attempt.rs:57`, and
- mark **C004** closed, pointing at this record.

---

## 13. Deviations from the plan

Every deviation is recorded here, with its reason.

| # | Deviation | Reason |
|---|---|---|
| D1 | **The authorization binds the principal (the session), not the exact turn.** §3.5 says the continuation must reuse `authorize`'s rule (the session *and* the turn must match the task's originating session and turn). The continuation binds the **session** instead (`src/agent/run_control.rs:272`). | §8 says explicitly that *"the parent's turn has ended. The continuation is authorized by the principal, not by a live turn."* The turn-level rule would make the continuation impossible in the turn that actually learns of the failure, which is the turn the plan requires it to work in. The turn-level rule is **untouched** and still governs `message`, `interrupt` and `cancel`; the continuation uses the same inputs and the same shape at the principal's granularity. |
| D2 | **The attempt row is written before the dispatch, not at the claim.** The claim writes only the continuation row; the attempt row is written immediately before the dispatch. | The attempt record is append-only. Writing the attempt row at the claim would leave a phantom attempt whenever a later step refuses (a transcript that fails re-verification, a dispatch that fails). Writing it before the dispatch keeps the record to attempts that were actually dispatched. |
| D3 | **A refusal releases the claim.** The plan does not say whether a refusal consumes the run's one continuation. | The bound is *one continuation per terminal failure*. A refusal is not a continuation, so consuming the bound on it would spend the parent's single chance on a run that could not be continued anyway. The release is what makes "a refusal does not consume the continuation" true. |
| D4 | **`RunControlService::authorize_continuation` is a sibling of `authorize`, not a change to it.** | The plan's stop condition forbids relaxing `authorize`'s rule. `authorize` is unchanged; the continuation's rule is a sibling that reads the same task and run records. The turn-level rule stays exactly as it was. |
| D5 | **The continuation's *job spec* (priority, resource request, timeout) is rebuilt from the spawn path's defaults rather than copied from the first attempt's job.** | The authority-bearing part of the first attempt — the **payload** — is copied verbatim from the attempt record. The rest of the job spec is the scheduler's admission, and the delegated spawn path always uses these same defaults. `JobSpec` is not `Serialize`, so the whole spec could not be stored verbatim without a wider change to the jobs crate (not this milestone's file set). |
| D6 | **The "superseded" case is defined as a run whose terminal outcome is not a failure.** | The plan names the case but not its definition. The definition used: the run reached a terminal outcome other than `Failed` (after `run_cancelled` is ruled out), so the failure the caller believes it is continuing is stale. It is recorded in `src/agent/run_control.rs:272` and in `architecture/agent.md`. |
| D7 | **The expiration window is 24 hours after the failure**, and the reconciliation's grace period is 60 seconds. | The plan requires an expired authorization to be a typed refusal and requires the reconciliation not to race a live dispatcher. Both windows are named constants (`crates/codegg-core/src/agent_run_attempt.rs:41` and `:48`). |
| D8 | **The provider's `circuit_open` class and the other non-transient provider classes map to `unclassified`** (not continuable). | The plan's taxonomy names five classes; the repository's `ProviderError::error_class()` has more. The extra classes are mapped to `unclassified`, which is **not** continuable, so the classification fails closed rather than widening the taxonomy without a decision. |
| D9 | **`src/tool/factory.rs` was edited** (one additive `with_transcript_store(...)` call) although C001 owns it. | The continuation reads the transcript back from the durable store, and the `task` tool is the only holder of that path. The change is additive and touches no C001 behaviour. Disclosed in §10. |
| D10 | **`tests/agent_loop/delegated_transcript.inc` (C002's member) was adapted**: its fixture now records the run's first attempt. | The provider entry now resolves the attempt's provenance from the attempt record, so a durable delegated run with no attempt record has no authorized attempt. The C002 fixture drives the worker directly, so it must record the attempt the executor would have recorded. The C002 assertions are unchanged; the six tests still pass. |
| D11 | **The seeded context is delivered as a separate content part ahead of the prompt**, not as a separate user message. | Two consecutive user messages are not portable across every provider adapter. The seeded context is still exactly the stored transcript, and it is still a separately identifiable part of the request, so the "the seeded context is exactly the stored transcript" property is directly assertable. |

---

## 14. Discharge of the outstanding items, and the daemon-reconciliation gap (orchestrator, 2026-10-24)

This milestone was written as *conditionally closed* on three centrally-owned
records, and it named one gap that was not centrally owned. The orchestrator
discharged all four on the merged tree after both round-2 workers had landed.

| # | Outstanding item | Discharge |
|---|---|---|
| 1 | **The comment-budget ratchet** (corpus 125 386 against a recorded ceiling of 124 716; the bank file is the orchestrator's). | Re-banked on the merged tree: **125 386** comment lines, **1 106 030** words, ratio **0.1330**; the bank diff is confined to the touched files plus the global block. The guard and its `--self-test` both pass. |
| 2 | **The recorded integration-test-file count** (`architecture/overview.md` recorded 342; the tree holds 346). | Reconciled on the merged tree: the measured value is **346**. `scripts/check_test_target_references.py` and its `--self-test` both pass. |
| 3 | **The static guard was not wired** into `scripts/verify.sh` / CI. | `scripts/check_delegated_continuation_authority.py` now runs in **both** surfaces, each with its `--self-test`. |
| 4 | **The reconciliation had no production caller**: `reconcile_continuations` existed and was tested, but "no daemon tick calls it yet". | **Closed by the orchestrator.** A startup reconciliation was added beside the existing startup reconcilers in `src/core/daemon_construct.rs` (the managed-worktree reconciliation and the writer-permit reconciliation), with its handle held on the daemon as `_continuation_reconcile_handle` (`src/core/daemon.rs`). It runs with a grace period of **zero**, because the daemon that made the claim is gone — there is no live dispatcher to race. It **surfaces** the claim as needing attention and **never** replays it, exactly as the plan requires. A startup repair is the right shape and not a periodic tick: the durable state it repairs is state a *previous* daemon left live, which is the same class as a worktree lease or a writer permit left live by a dead daemon. |

With those four discharged, `bash scripts/verify.sh quick` exits **0** on the
merged tree (119 steps, "==> Quick verification passed."), the whole-workspace
clippy is clean with `-D warnings`, and this milestone is **closed**.

**The `src/tool/factory.rs` edit (D9) stands** and is recorded here: one
additive `with_transcript_store(...)` call in a file C001 owns, made because
the continuation reads the transcript back from the durable store and the
`task` tool is its only holder. It touches no C001 behaviour.

The two `core_daemon` failures observed during this milestone are
**pre-existing and environment-dependent** — they register `/tmp` as a
workspace root, and `canonicalize_workspace_root` refuses a symlinked root
while `/tmp` is a symlink on macOS. The orchestrator confirmed the same call
is present at the pre-change commit `220b763a0`.
