# ADR-0039: Queue Advancement Wakeup Authority

Status: **proposed**

Date: 2026-10-18 (authored). **Acceptance is a maintainer action** — see
[Acceptance](#acceptance).

**Supersedes nothing.** ADR-0038 decided *as whom* a due entry is submitted. It
explicitly left *when* an entry becomes due undecided, and
`plans/implementation/user-prompt-queue/007-delayed-queue-delivery.md` (M007) is
blocked on that question. This ADR selects **Option A**, the bounded
queue-scoped advance tick, and authorizes M007.

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`

Affected subsystem roadmap:

- `plans/subsystems/user-prompt-queue-roadmap.md`

Related ADRs:

- `plans/adrs/ADR-0035-durable-user-prompt-queue-ownership.md` (accepted)
- `plans/adrs/ADR-0036-mid-turn-prompt-steering-delivery-contract.md` (accepted)
- `plans/adrs/ADR-0038-queue-delivery-execution-authority.md` (accepted)

Related milestone:

- `plans/implementation/user-prompt-queue/007-delayed-queue-delivery.md`

## Context

### The roadmap's stated basis was false

The user-prompt-queue roadmap gave delayed delivery this basis:

> Advancement is evaluated at an existing periodic observation point rather than
> by a new timer thread, so a `1 hr` delay introduces no new wakeup source and no
> new process-global scheduler.

Planning M007 measured this and **falsified** it. There is no existing periodic
observation point for the queue:

- The daemon's periodic loops are the 300 s projection replay **retention** tick
  (`src/core/daemon_construct.rs`), an opt-in provider refresh, and opt-in TTS.
  The first is a projection-retention service, spawned only when the projection
  seam is enabled. None of them observes a session's queue.
- `submit_queued_prompt_turn` had exactly two permitted callers — the
  post-terminal wake and the bounded restart twin — enforced by
  `scripts/check_queue_delivery_callsites.py`.
- Neither fires when a session is idle with a scheduled entry and no turn
  activity.

So the consequence is concrete. With due-ness enforced in the store query alone,
an entry scheduled for 09:00 that becomes due at 09:01 on an idle session is
delivered **when something else next happens** — possibly much later, possibly
never within the session. That satisfies "does not fire early" and fails "a user
can leave an instruction for later".

### Why this needs an ADR at all

M003's delivery entry was deliberately fenced to two callers. Adding a third is
an execution-authority change, not a feature: a new caller of the daemon's
submit path is exactly the surface that guard exists to keep small. `AGENTS.md`
requires such a widening to be a **recorded decision**. That recording is this
ADR.

## Decision

**Option A — a bounded, queue-scoped advance tick.**

One daemon task computes the instant at which the next scheduled entry becomes
due, sleeps exactly until then, and advances every session holding a due entry
through the existing per-session delivery entry.

### Options considered

| Option | Shape | Why not |
|---|---|---|
| **A. Bounded queue-scoped advance tick** | One task; next wake computed from the earliest due time; parks when nothing is scheduled | **Selected.** The only option that delivers the milestone's promise without coupling to an unrelated subsystem. |
| **B. Reuse the 300 s projection tick** | Hook the existing maintenance loop | Wrong owner (projection retention), conditionally spawned behind a feature flag, 5-minute granularity, and it silently couples queue delivery to a setting that has nothing to do with the queue. |
| **C. Accept late delivery** | Store due-ness only; no new trigger | Zero new machinery, but "1 hr" becomes "1 hr, or whenever this session next does something" — the roadmap's own UX premise, broken. |

### The park condition is load-bearing

The task must **park** when no session has a scheduled entry, and it must do so
with **no timer armed at all**. This is the property M003 refused to give up, and
it is the reason the wake is computed rather than polled.

A parked task has no deadline to wait for, so it can only learn that a schedule
appeared from the write that created one. `add` and `edit` therefore signal a
`tokio::sync::Notify` after a successful write. This is deliberately **not** a
polling interval: polling for "has anything been scheduled yet" would reintroduce
exactly the unconditional wakeup this design exists to avoid, keeping an idle
daemon awake forever for an empty queue.

`Notify::notify_one` stores a permit when no one is waiting, so a schedule
created *during* an advance pass is not lost. The failure this rules out is a
prompt scheduled for 09:00 sleeping through its own wake because its signal
landed while the task was mid-pass.

## Consequences

### What the tick is not

- **It is not a second delivery authority.** The tick only *nominates* sessions.
  It reads no prompt text, resolves no owner, and admits no turn. Everything
  after nomination goes through `submit_queued_prompt_turn`, which re-checks
  ownership, the `status = 'pending'` at-most-once fence, and current
  authorization.
- **It is not a second scheduler.** No executor, no scheduler permit, no job
  template. ADR-0035 rejects the `JobTemplate` model explicitly.
- **It changes *when*, not *as whom*.** ADR-0038 is untouched. Delivery still
  runs as the reconstructed queue owner, with no synthetic principal and no
  `is_controller_bypass`.

### Bounded, and bounded on purpose

The sweep query is limited by `USER_PROMPT_QUEUE_MAX_RECONCILE`, and it returns
**session ids only**. A backlog larger than the bound drains over successive
passes; correctness never depends on seeing every session at once, because each
pass re-runs the query. This is also the guard against the sweep becoming a way
to enumerate sessions across projects.

### Delivery is late, not early, when the daemon is down

A due time is a floor, never a deadline. If the daemon is not running across the
due instant, the entry is delivered on restart — late — and only then. The
promise is a floor the daemon can keep when it is absent, not a wall-clock
guarantee it cannot make.

### Consequences of the wakeup widening

- `PERMITTED_CALLERS` in `scripts/check_queue_delivery_callsites.py` goes from
  two entries to three, with the rationale recorded on the constant.
- The guard's `--self-test` now injects a **fourth** caller and proves it is
  still rejected. A guard widened without re-proving its refusal is a guard that
  constrains nothing.
- ADR-0038's "exactly two" language is superseded for the caller set only. Its
  decision about *authority* stands.

### Rejected alternatives worth recording

- **A `CHECK`-enforced "scheduled" status.** v80's `status` `CHECK` cannot be
  `ALTER`ed by SQLite without a table rebuild, and a scheduled status would also
  have split `pending` into two states that `OPERABLE_STATUS_PREDICATE` and the
  claim query both name. A nullable `run_at_ms` needed no `CHECK` at all,
  because a nullable column with no sentinel admits every value the writer can
  produce.
- **Backfilling `0` for existing rows.** `0 <= now` is true, so delivery would be
  unaffected — and every pre-existing entry would render as "scheduled", because
  the user's guidance is explicitly to correct measured numbers rather than drop
  inconvenient ones. `NULL` is the honest representation of "no due time", and
  the v81→v82 upgrade test asserts it.
- **A second duration parser.** `src/background_task_migration.rs::parse_duration`
  already defines the repository's grammar (`30s`, `5min`, `10m`, `2h`, `1d`).
  A second grammar is how a bare `5` becomes five seconds in one path and five
  minutes in another. The roadmap's example spelling `1 hr` **does not parse** and
  must not appear in copy, tests, or docs as though it did.

## Acceptance

This ADR is **proposed**, not accepted. Per the M007 plan's stop conditions, an
acceptance is a maintainer action: the implementation of the wakeup is
authorized by that acceptance, not by this document's existence.

On acceptance, this ADR's status becomes **accepted**, the date line records the
acceptance, and the following must be updated in the same change:

- `plans/subsystems/user-prompt-queue-roadmap.md` — the Milestone 7 row and the
  §5 basis statement, which still claims an existing periodic observation point.
- `plans/registry.md` — the M007 row.
- `plans/implementation/user-prompt-queue/007-delayed-queue-delivery.md` — the
  status line.