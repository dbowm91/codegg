# User Prompt Queue Milestone 007 — Delayed Queue Delivery (Timers)

Status: **closed** — implemented 2026-10-18. Closure record:
`plans/closure/user-prompt-queue/007-status.md`.

The §2 decision was **taken: Option A**, a bounded queue-scoped advance tick,
recorded in `plans/adrs/ADR-0039-queue-advancement-wakeup-authority.md` (**accepted
2026-10-18**). §14's stop conditions no longer apply; where the implementation
departed from the plan, the closure record names the departure and its reason.

Repository baseline: `18c3dca92099fe2de09f875731530348fb76b68f` plus the
uncommitted M001–M006 tree. §3 records measured facts at this baseline only; they
were re-measured and all held.

Source roadmap:
`plans/subsystems/user-prompt-queue-roadmap.md#milestone-7--delayed-queue-delivery`

Long-term requirements:

- `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-multi-session-tui`

Applicable ADRs:

- `plans/adrs/ADR-0035-durable-user-prompt-queue-ownership.md` (accepted) — **quotes
  this milestone's central UX decision as deferred to it**, §3.4.
- `plans/adrs/ADR-0036-mid-turn-prompt-steering-delivery-contract.md` (accepted) —
  discharges the "send now" copy boundary.
- `plans/adrs/ADR-0038-queue-delivery-execution-authority.md` (accepted) — owns *as
  whom* a due entry is submitted; M007 changes only *when* it becomes due.
- `plans/adrs/ADR-0039-queue-advancement-wakeup-authority.md` (accepted) — owns
  *when* a due entry becomes eligible. Written by M007; accepted 2026-10-18.

Primary class: **capability**.

## 1. Objective

Let a user say "run this in an hour" and have the entry become eligible at that time,
without introducing a second scheduler, without delivering early, and without losing the
due time across a daemon restart.

## 2. Why this milestone is *not* ready

The roadmap's stated basis for this milestone is false, and planning it falsified it:

> **Delayed delivery.** Entries carry an optional due time. Advancement is evaluated
> at an existing periodic observation point rather than by a new timer thread, so a
> `1 hr` delay introduces no new wakeup source and no new process-global scheduler.

**There is no existing periodic observation point for the queue.** Measured at this
baseline:

- The daemon's only `tokio::time::interval` loops are the **projection replay
  maintenance tick at 300 s** (`src/core/daemon_construct.rs:166-174`), opt-in provider
  refresh, and opt-in TTS. The first is a projection-retention service, spawned only
  when the projection seam is enabled, and it is not a session or queue observation
  point. M003 recorded the same finding from the other direction.
- M003's delivery entry `submit_queued_prompt_turn` has **exactly two permitted
  callers**, enforced by `scripts/check_queue_delivery_callsites.py`: the post-terminal
  wake (`deliver_next_queued_prompt`) and the bounded restart twin
  (`reconcile_stranded_queue_deliveries`).
- Neither fires when a session is idle with a scheduled entry and no turn activity.

So the consequence is concrete: with due-ness enforced in the store query alone, an
entry scheduled for 09:00 that becomes due at 09:01 with an idle session is delivered
**when something else next happens** — possibly much later, possibly never within the
session. That satisfies "does not fire early" and fails "a user can leave an instruction
for later".

**The named decision — who wakes a due entry.** Three options, with costs:

| Option | Shape | Cost |
|---|---|---|
| **A. Bounded queue-scoped advance tick** (recommended) | One daemon task that advances due entries on a short interval, **self-disabling while no session has a scheduled entry**, calling the existing delivery entry | Adds a wakeup source. Requires widening `PERMITTED_CALLERS` in the callsite guard — a **recorded decision**, per this repository's standing rule — plus the ADR-0038 execution-authority review, since a new caller runs the delivery path |
| **B. Reuse an existing periodic loop** | Hook the 300 s projection-maintenance tick | Wrong owner (projection retention), conditionally spawned, 5-minute granularity, and it silently couples queue delivery to a feature flag |
| **C. Accept late delivery** | Store due-ness only; no new trigger | Zero new machinery, but "1 hr" becomes "1 hr, or whenever this session next does something", which is the roadmap's own UX premise broken |

Option A is recommended: it is the only one that delivers what the milestone promises,
and its self-disabling condition keeps it from being a general poller — the thing
M003 explicitly refused. If the maintainer selects A, a new ADR numbered **0039** (queue advancement
wakeup authority) is the right discharge vehicle, because a new caller of the
delivery entry is an execution-authority change. It is referred to by number here
rather than by path because it **does not exist**: **this plan does not draft it**
— an acceptance is a maintainer action (§14), and `scripts/check_adr_integrity.py`
correctly refuses a link to a file that has not been written.

Everything else below is settled and ready to implement the moment that one decision
lands.

## 3. Current implementation evidence

### 3.1 The column is reserved, and it is genuinely absent

`migrate_v80` carries a comment stating `run_at_ms` is *deliberately absent*
(`crates/codegg-core/src/session/schema.rs:3364`) because nothing in that milestone
wrote it. v80 is shipped and is **never edited**; M007 adds the column with an in-place
`ALTER TABLE … ADD COLUMN`, which is **v82** (`STORAGE_LAYOUT_VERSION` is currently
**81**, migrations end at v81). SQLite cannot `ALTER` a `CHECK`, and `run_at_ms`
nullable needs none.

### 3.2 A duration parser already exists — reuse it, do not write a second

`src/background_task_migration.rs:11-34` is the repository's `parse_duration`. Its
accepted grammar is `30s`, `5min`, `10m`, `2h`, `1d`, and nothing else; it returns
`None` for anything else, including **a bare number** and **`1 hr`** (the suffix strip is
`'h'`, so `1 hr` ends in `r`).

Two consequences, both load-bearing:

- A bare `5` must be **refused**, not guessed as seconds. A silent guess is how "5"
  becomes five seconds.
- The roadmap's example spelling `1 hr` **does not parse** and must not appear in copy,
  tests, or docs as though it did. Use `1h`.

Reusing this parser keeps one duration grammar in the repository. Adding a second
parser is a stop condition (§14).

### 3.3 The wakeup gap is the blocker, not a detail

`claim_next_for_delivery` selects
`WHERE status = 'pending' AND expires_at_ms > ? ORDER BY ordinal ASC LIMIT 1`
(`user_prompt_queue.rs:1401-1405`). Adding due-ness here is one predicate and is
correct for *not-early*. It cannot make an entry *become* due — see §2.

The store comment on the same function states the intent this milestone must preserve:

> Ordering by ordinal rather than by age is what makes a queue a queue. A trigger that
> fires out of order must not reorder the user's work.

### 3.4 Reorder-versus-timer precedence is explicitly deferred to this milestone

ADR-0035 states:

> Per-entry timers are specified by the roadmap, but the interaction between reordering
> and timer ordering is a UX decision reserved to the milestone that introduces both.

**Decision: `run_at_ms` gates eligibility; `ordinal` orders among eligible entries.**

Concretely, the delivery predicate becomes
`status = 'pending' AND (run_at_ms IS NULL OR run_at_ms <= now)`, still
`ORDER BY ordinal ASC LIMIT 1`. Reordering therefore **cannot** invert timer
precedence, because a reorder only permutes entries that are already due; an entry not
yet due is invisible to the ordering entirely. No refusal is needed for scheduled
entries.

This takes the roadmap's second offered branch — *"either reorder is refused for
scheduled entries or the ordering rule is explicit and tested"* — and it is the branch
that does not degrade the M005 capability.

### 3.5 Expiry interacts with due time, and the interaction is a refusal

An entry carries `expires_at_ms`. A `run_at_ms` later than its own expiry would become
due only after it has expired — a scheduled prompt that can never run, silently.

**Rule: refuse a due time at or beyond the entry's own expiry**, at add time and at edit
time, with the same typed-refusal shape every other bound uses (never truncation, never a
silent clamp). The concrete horizon must be read from the existing add path and named
in the copy rather than invented here.

### 3.6 M005's displacement defect is visible from this milestone too

An entry whose delivery was released may sit outside the dense ordinal band
(§3.2 of M005's closure record). With timers added, that entry is also a candidate for
late delivery, so the two defects compound in the user's experience: back of the queue
**and** late. C001 owns the ordering half; this milestone does not work around it.

## 4. Invariants that must not regress

1. **Nothing is delivered early.** A due time is a floor, never a hint.
2. **A due time survives restart.** It is a durable column, and it is read from storage
   on every delivery attempt — never from a frontend copy or an in-memory timer.
3. **One scheduler.** No second executor, no scheduler permit, no job template (ADR-0035
   rejects the `JobTemplate` model explicitly).
4. **Delivery authority is unchanged.** M007 changes when an entry becomes eligible, not
   as whom it is submitted; ADR-0038 still governs, and no synthetic principal or
   `is_controller_bypass` appears.
5. **Reorder cannot invert timer precedence** (§3.4).
6. **An unparseable duration is refused**, never defaulted.
7. **A due time past expiry is refused**, never clamped.
8. **Recurring schedules and calendar triggers stay out.**
9. **The due-time predicate cannot make delivery early under concurrency**: a tick that
   fires at the due instant and a claim that lands a millisecond later must agree.

## 5. Scope

### In scope

- `run_at_ms` as a nullable column in **v82**, plus the layout bump.
- Capture at add time; editable thereafter.
- Duration parsing and display through the existing grammar.
- Due-ness in the store's delivery predicate.
- One wakeup, per the §2 decision.
- Explicit, tested reorder-vs-timer behaviour.
- Refusal of a due time past expiry.

### Explicitly out of scope

- **Recurring schedules, cron, and calendar triggers.**
- **Any change to M006's steering path.**
- **Any new status value** and any change to v80 or v81.
- **Editing another session's queue**, and any observer path.
- **Desktop** (M008).
- **Fixing the release-displacement ordering defect** (C001).
- **A human-readable relative-time renderer** beyond showing the due time and remaining
  time; no new time-formatting framework.

## 6. Required production changes

### Storage and migrations

- **v82**: `ALTER TABLE user_prompt_queue_entry ADD COLUMN run_at_ms INTEGER` —
  nullable, no `CHECK`, no index rebuild. Bump `STORAGE_LAYOUT_VERSION` to 82; two
  existing assertions cover it
  (`scripts/check_project_catalog_invariants.py` and `tests/storage_migrations.rs`).
- A **v81 → v82** upgrade test asserting existing entries read as "no due time" rather
  than as a sentinel.
- Add `run_at_ms` to `ENTRY_COLUMNS` and to the record/live-entry projections.
  Consider an index on `(session_id, status, run_at_ms)` for the advance sweep, sized
  against `USER_PROMPT_QUEUE_MAX_RETAINED_PER_SESSION`.
- `run_at_ms` is **not** prompt text and carries no secrecy requirement, but it is
  user intent: it is never logged verbatim beyond entry identity and due time.

### Core / domain

- `add` and `edit` accept an optional `run_at_ms`; both validate against the entry's
  expiry and return a typed refusal otherwise.
- `claim_next_for_delivery` gains the eligibility predicate.
- The advance sweep is a bounded query over sessions with a due scheduled entry, so an
  idle daemon with nothing scheduled does no work.

### Protocol and DTOs

- `run_at_ms` is added to the existing queue request/response DTOs as an **optional**
  field. Because `#[serde(default)]` is the established convention for projection
  fields here (M001's projection work), a pre-M007 frontend reads it as absent, which
  is exactly "no due time".
- **A frontend that predates v82** and sends no due time is unaffected; the entry
  behaves as an immediate entry. No capability is silently lost in either direction.

### Runtime and concurrency

- The wakeup per §2's decision. Under Option A it is a **queue-scoped** task that
  computes its next wake from the earliest due time and **parks** when nothing is
  scheduled — not a fixed-interval poller that ignores its own work.
- `scripts/check_queue_delivery_callsites.py` must be **widened deliberately**: the new
  caller is added to `PERMITTED_CALLERS` with a comment, and the guard's `--self-test`
  is re-run to prove a *fifth* caller still fails. `AGENTS.md` is explicit that
  widening this set is a recorded decision, never a quiet third call site.
- The advance path reuses `deliver_next_queued_prompt`'s existing discipline: one entry
  per claim, refusals released or made terminal by typed class, never by error wording.

### Frontend surface

- Capture at add: a timer field in the add step, or a separate modifier key. Whichever
  is chosen, the browsing hint must advertise it and the text-step hint must change
  with it, as M005 established.
- Display: the due time and the remaining time on the row, and the same for the
  composer line.
- Editing an existing due time is a first-class operation, not an implicit re-add.
- An entry that is not yet due is visually distinguishable from one that is, and the
  distinction is **not** colour-only.

### Security and authorization

- Unchanged from M002b/M003: mutation by the durable owner binding, delivery by the
  ADR-0038 principal. The advance tick runs as the daemon but delivers **as the owner**,
  exactly as the existing pump does.
- The advance sweep must not become a way to enumerate sessions across projects.

### Documentation and static guards

- `architecture/session.md` + the `session-storage` skill: the column, the migration,
  and the eligibility rule.
- `architecture/core.md` + the `core` skill: the wakeup owner.
- `docs/tui.md`: how to schedule, what the grammar is, and that `1h` is the spelling.
- The callsite guard's comment and the roadmap's §5/§7 text corrected to match the
  decision actually taken.

## 7. Ordered work packages

### Work package A — The migration and the column

*Intent:* v82, the bump, the upgrade test.

*Acceptance evidence:* a v81→v82 upgrade test; both layout assertions; `verify.sh quick`.

### Work package B — Eligibility in the store

*Intent:* due-ness in the delivery predicate, plus the expiry refusal on add/edit.

*Acceptance evidence:* store tests proving a not-yet-due entry is never claimed, a due
entry is claimed at its instant, a past-expiry due time is refused on both add and
edit, and an existing immediate entry is unaffected.

### Work package C — The wakeup

*Intent:* the §2 decision, implemented and guarded.

*Acceptance evidence:* a test proving a due entry with an otherwise idle session is
delivered without any turn activity; the callsite guard widened with a comment;
`--self-test` re-run and proving a fifth caller fails; a test proving the task parks
when nothing is scheduled.

### Work package D — Authoring surface

*Intent:* capture, display, edit.

*Acceptance evidence:* integration tests driving the real key path; per the M005
precedent, the hint follows the step and names only implemented keys.

### Work package E — Copy and docs

*Intent:* grammar, refusal sentences, and the reorder-vs-timer rule stated in the user
guide.

*Acceptance evidence:* a test asserting `1 hr` appears in no hint or toast, that a
bare number is refused rather than guessed, and that no copy promises a specific
delivery time.

## 8. Failure, cancellation, restart, and contention semantics

- **Clock or due-time in the past at commit:** eligible immediately; that is a due time
  the user already passed, and refusing it would be pedantry.
- **Daemon down across the due instant:** the entry is delivered on restart, late, and
  only then. The due time is a floor, never a deadline that fires while the daemon is
  absent — a promise the daemon cannot keep when it is not running.
- **Two frontends, or a tick racing a user mutation:** the claim's
  `status = 'pending'` fence and its single-winner `rows_affected == 1` hold; at most
  one delivery results.
- **An entry edited while due-but-unclaimed:** it takes the new due time; the next
  attempt reads the current row, never a cached one.
- **An entry deleted or already delivered when the tick fires:** the sweep selects live
  pending rows only; there is nothing to deliver and no error.
- **Tick failure:** logged with entry identity and reason, never text, and the next tick
  retries. A failing advance must not wedge the pump.

## 9. Compatibility and migration

- **v82**, additive, nullable, no table rebuild. v80 and v81 untouched.
- `STORAGE_LAYOUT_VERSION` 81 → 82.
- Projection fields default, so an older frontend reads an absent due time as "none".
- **Migration-number contention:** v82 is this milestone's. If C001's release-order fix
  is authorized and lands first, the two swap (v82/v83) and every document naming the
  number is corrected together — including the v80 comment at `schema.rs:3364` and the
  ADR/registry/roadmap references. This is a landing-order fact, not a blocker.

## 10. Required tests

### Migration tests

- v81 → v82 upgrade; `run_at_ms` null for pre-existing rows; layout constant equals the
  highest migration.
- A v82 pool round-trips a due time through reopen.

### Store tests

- Not-yet-due is never claimed, at and around the boundary millisecond.
- Due is claimed at its instant.
- Due time at or beyond expiry refused on add and on edit.
- An immediate entry behaves exactly as before.
- Delete and reorder still renumber correctly with scheduled entries present.

### Wakeup tests

- A due entry with an idle session and no turn activity is delivered.
- Nothing scheduled ⇒ no work (assert the park, not just the absence of a delivery).
- The daemon restart case: due time in the past, delivered once after restart.
- The callsite guard's `--self-test` proving a fifth caller fails.

### Integration tests

- Add with a duration; edit the duration; the row shows it.
- An unparseable duration is refused with a reason and the text restored.
- Reorder cannot make a not-yet-due entry deliver before its time.

### Guards proven non-vacuous

- Remove the eligibility predicate and the not-early test must fail.
- Add a fourth delivery caller without updating the guard and
  `check_queue_delivery_callsites.py` must fail.

## 11. Required verification commands

```bash
cargo test -p codegg-core --test user_prompt_queue_mutation
cargo test -p codegg-core --test user_prompt_queue_delivery
cargo test -p codegg-core --test storage_migrations
cargo test -p codegg --lib -- dialogs::queue commands::queue
cargo test --test user_prompt_queue_box

python3 scripts/check_queue_delivery_callsites.py
python3 scripts/check_queue_delivery_callsites.py --self-test
python3 scripts/check_project_catalog_invariants.py

cargo fmt --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
```

`CARGO_BUILD_JOBS=2`. Never `--all-features`.

## 12. Documentation updates

- `architecture/session.md`, `session-storage` skill — the column and migration.
- `architecture/core.md`, `core` skill — the wakeup owner as decided.
- `architecture/tui.md`, `tui` skill, `docs/tui.md` — authoring, grammar, display.
- Roadmap §5/§7/§10 corrected from "an existing periodic observation point" to the
  decision actually taken, with the falsification recorded.
- Registry and this plan's status line updated together.

## 13. Acceptance criteria

1. A user can add an entry with a due time in the accepted grammar and see it.
2. It is never delivered before that time, including across a restart.
3. A due entry with an idle session and no turn activity is delivered without waiting
   for unrelated activity.
4. A due time at or beyond the entry's expiry is refused with a reason.
5. An unparseable or bare-number duration is refused, never guessed.
6. Reordering cannot cause a not-yet-due entry to be delivered early.
7. No new timer thread polls unconditionally; the wakeup parks when nothing is
   scheduled.
8. Delivery still runs as the queue owner under ADR-0038, and no synthetic principal or
   `is_controller_bypass` appears.
9. Both guards in §10 are proven non-vacuous by injection, and the injections are
    reverted.
10. `scripts/verify.sh quick`, clippy `-D warnings`, `cargo fmt --check`, and the two
    layout assertions pass.

## 14. Stop conditions

Report rather than improvise if:

- **The §2 decision is not made.** Do not build a timer speculatively; that is the exact
  mistake the roadmap's falsified premise would produce.
- **A second duration parser seems more convenient.** Reuse `parse_duration`; a second
  grammar is a stop-and-report.
- **The due time appears to need cron, recurrence, or a calendar.** Out of scope;
  these are separate milestones.
- **A new `status` value seems required** (for example "scheduled"). v80's `CHECK`
  cannot be `ALTER`ed; report the rebuild cost instead.
- **The wakeup appears to need the durable job scheduler.** ADR-0035 rejects modelling a
  prompt as a `JobTemplate`.
- **The callsite guard would be widened quietly.** It is a recorded decision; a third
  caller added without the guard update and its rationale is a defect.

## 15. Closure evidence required

- Requirement-to-evidence matrix over the ten acceptance criteria.
- Real command output with counts for §11, labelled local.
- Both injection transcripts.
- The recorded decision that discharged the §2 blocker, with its ADR if one was raised.
- A registry unblock audit.

## 16. Handoff notes

Do not start work package A's follow-on work until §2 is decided — the migration is
safe to land either way, but the wakeup design is not, and landing the column without
the decision recreates the claim-without-a-hook defect this repository treats as a bug.

The single most likely failure mode is shipping `run_at_ms` with due-ness enforced in
the query and no wakeup at all, which satisfies every test in §10 except the one that
matters to a user: the entry sits there until something unrelated happens.