# User Prompt Queue and Turn Contention Roadmap

Status: active

Repository baseline: `18c3dca92099fe2de09f875731530348fb76b68f`
(`feat(tui): panel chrome, keymap adoption, and derived theme selection (M002a-M003)`)
Re-inspect before each implementation pass; §4 records measured facts at this
baseline only.

Long-term references:

- `plans/000-long-term-specification.md#4-architectical-principles`
- `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`
- `plans/000-long-term-specification.md#15-read-only-session-observation`
- `plans/000-long-term-specification.md#22-audit-architecture`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-multi-session-tui`

Related ADRs:

- `plans/adrs/ADR-0035-durable-user-prompt-queue-ownership.md` (accepted 2026-10-18)
- `plans/adrs/ADR-0036-mid-turn-prompt-steering-delivery-contract.md` (accepted 2026-10-18) — fixes the steering guarantee as **acceptance, not delivery**, forbids "force" in copy, and distinguishes `TurnSteer` from the agent-loop follow-up drain
- `plans/adrs/ADR-0038-queue-delivery-execution-authority.md` (accepted 2026-10-18) —
  raised by the M002b closure audit; owns **as whom** a queued entry is
  submitted, and selects **Option B** (a named `submit_queued_prompt_turn`
  entry; attribution from the owner row plus team state; **no migration**)
- `plans/adrs/ADR-0037-idle-session-queue-mutation-authority.md` (accepted 2026-10-18) —
  raised by the ADR-0035 acceptance audit; selects **Option B** (durable,
  session-scoped queue-owner binding) and adopts the M002a/M002b split

Predecessor and adjacent workstreams, neither reopened by this roadmap:

- `plans/subsystems/tui-panel-surface-convergence-corrective-addendum.md` — owns the
  shared box chrome, keymap-hint, and geometry seams this roadmap's dialogs consume.
  Its M004 plan is the owner of the shared chrome/geometry helper.
- `plans/subsystems/tui-subagent-and-job-visibility-roadmap.md` — owns the working
  strip's subagent and job rows. This roadmap adds no rows and changes no row
  height; it only appends a suffix to the existing row 0.
- `plans/subsystems/tui-project-sessions-roadmap.md` and its convergence
  addendum — own session selection and frontend convergence.

## 1. Purpose and ownership boundary

This subsystem owns **user-authored prompt text that has been composed but not yet
delivered as a turn**. It owns the storage, ordering, expiry, and authority of that
text between composition and delivery, and the user-facing surfaces that manage it.

It **consumes**, and must not redefine:

- turn admission, the controller lease, and submission receipts owned by
  `crates/codegg-core/src/session_control.rs`;
- the `CoreRequest`/`CoreResponse` protocol surface owned by `codegg-protocol`;
- box chrome, keymap hints, and geometry owned by the panel-convergence workstream;
- the working strip layout and its reserved row 0 owned by the TUI render path;
- agent-loop internals, including the turn-boundary follow-up drain.

It **must not own**:

- a second submission path around turn admission. Every queued prompt becomes a
  turn through ordinary `TurnSubmit`;
- any authority to cancel or restart a turn in order to deliver a message
  (see ADR-0036);
- per-attachment or per-frontend private state that outlives the process;
- the durable job scheduler's execution model.

## 2. Work classification

### Invariants

- A queued prompt is delivered **at most once**. Duplicate delivery is a
  correctness defect, not a retry outcome.
- A queued prompt is **never silently lost**: it is delivered, visibly failed, or
  expired with a recorded reason.
- Queue order is a **daemon-owned total order**. Two attached frontends cannot
  interleave mutations into an inconsistent sequence.
- Queue mutation is authorized by the **same controller predicate** as turn
  admission. Observers and non-controller contributors are refused.
- Queue entries reach execution **only** through ordinary `TurnSubmit` admission,
  inheriting the controller lease, the receipt, and the interrupted-no-replay rule.
- A refused submit must **not** corrupt active-turn state, and must **not** destroy
  the user's composed text.
- Queue growth is bounded by per-entry length, entry count, and expiry.

### Capabilities

- Compose a prompt while a turn is active and have it retained rather than lost.
- See, at a glance, that prompts are waiting and how to reach them.
- List queued prompts, select one, and delete it.
- Add a prompt to the queue, with an optional delayed-delivery time.
- Edit a queued prompt in place before it is delivered.
- Reorder queued prompts, including relative to each other.
- Deliver the next queued prompt as a real turn when the session is free.
- Inject a queued prompt into the currently running turn, best-effort, with an
  honest failure report.

### Infrastructure

- The durable queue table, its migration, and its bounded status/expiry model.
- Typed core requests and responses for queue operations.
- The queue projection consumed by frontends.
- The observation and audit surface for queue mutation.

Infrastructure is **not** user-visible capability. The migration and the request
surface are not evidence that a user can queue a prompt; only M004 and later
provide that.

### Polish

- Empty-state copy that names how to add the first entry.
- Compact row rendering for long prompts.
- Per-row delivery status and timer affordances.

Polish follows the correctness boundary of the capability it serves and is not
promoted ahead of it.

## 3. Non-goals

- **Not** a parallel scheduler. Queued prompts do not acquire scheduler permits,
  executor ownership, or workspace execution context.
- **Not** a branch of the durable job scheduler's execution model. Reusing its
  storage and trigger machinery is in scope; modelling a prompt as a `JobTemplate`
  is explicitly rejected in ADR-0035.
- **Not** a message or chat system. No assistant-authored messages, no agent-to-agent
  queueing, no cross-session queue.
- **Not** a turn-cancellation gesture. "Send now" does not cancel or restart.
- **Not** a multi-user coordination feature. The controller lease decides who may
  mutate; this does not build shared editing or presence for the queue.
- **Not** a workspace or project-scopoped feature. Entries are session-scoped.
- **Not** retroactive. Text already lost to the pre-M001 defect is not recoverable
  and no repair path is provided.
- **Not** a change to working strip row 0 reservation. This roadmap appends to
  row 0 and must not add a row, gate row 0 on queue state, or reflow the
  transcript.

## 4. Current state

Measured at `18c3dca92099fe2de09f875731530348fb76b68f`. **M001 is closed**
(`plans/closure/user-prompt-queue/001-status.md`); the paragraphs below marked
*pre-M001* describe the baseline that motivated this subsystem and are retained as
the reason the milestone exists, not as the current behaviour.

**A prompt composed during a turn was lost.** *(pre-M001, now fixed)* The
composer clears before the transport decision is made, and the refusal path did
not restore it. The `turn_already_active` refusal was reported and the composer
was released, so the user was told why and left with an empty box. This was the
defect that motivated the subsystem. Restoration now runs through the single
`PromptState::restore_prompt_with_stash` shape for every recoverable refusal,
session-create failure, and reconnect.

**The refusal path also corrupted active-turn state.** *(pre-M001, now fixed)*
The submit-refusal arm reported every failure through `fail_unstarted_turn`,
which cleared `working_since`, cleared `active_turn_id`, set
`SessionStatus::Error`, and stopped the thinking indicator — including when the
refusal was `turn_already_active` and a turn was in fact still running on the
daemon. The frontend therefore reported "no active turn" while one existed. Any
queue UI that consults turn state to choose between queue and steer would have
read a state this arm just invalidated. Contention now takes
`App::fail_contended_turn`, which leaves turn state untouched and reports a
warning toast rather than latching a turn error — latching would replace the live
turn's row-0 status line, which gives `last_error` absolute precedence.

**Submission has a single in-flight slot and no shared completion guard.**
`pending_turn_submission` is one `Option`, and the submit path spawns a raw task
that hand-rolls a currency check at the receive site instead of using the shared
`spawn_tui_task` plus request-id-keyed completion guard that the TUI's async
handler convention mandates. A queue makes concurrent in-flight submissions the
normal case, which is exactly the condition that guard exists for.

**Steering exists and is unreachable.** `CoreRequest::TurnSteer` is implemented in
the daemon turn dispatcher and forwards into a bounded per-turn channel on the
active turn handle, gated by the controller predicate. No TUI code path issues it.
Its failure modes — full channel, closed receiver, unsupported turn — have never
been product-reachable and therefore have no product-visible behavior.

**The durable controller lease does not exist while a session is idle.**
`SessionControllerStore::acquire` validates a non-empty `turn_id`, and `release`
`DELETE`s the row, so `session_turn_controller` exists only while a turn runs.
`CoreDaemon::controller_lease_for` returns `None` in between and
`CoreDaemon::check_turn_controller` then refuses *every* principal — including the
legitimate owner. The single idle-passing path is the `is_controller_bypass`
early return, which is `is_local_owner_broad`: an identity comparison against
`LOCAL_OWNER_PRINCIPAL_ID` that is not bound to a session. This is why ADR-0035's
clause "queue mutation is authorized by the same controller predicate that
governs turn admission" is unsatisfiable *between turns* — which is exactly when
a user opens the queue. It forced the M002a/M002b split and `plans/adrs/ADR-0037-idle-session-queue-mutation-authority.md`.

**A different mechanism already drains prompts into the agent loop.**
`AgentLoop::drain_follow_up` consumes follow-up prompts with a non-blocking receive
at turn boundaries and documents that late arrivals are not processed by the same
run. It is agent-internal and unrelated to user composition. See ADR-0036.

**The storage substrate is ready to extend.** Migrations are monotone up to **v79**
and `STORAGE_LAYOUT_VERSION` is **79**; the next additive migration is v80 and the
constant must match, which two checks assert. `human_shell_promotion` is an
existing durable staged-prompt table with a 24-hour TTL, pending/expired/promoted
status, expiry, and binding to a turn through a consumed-turn column — the closest
existing structural analogue.

**The status line is a single reserved row with an established precedence.**
Working strip row 0 is reserved unconditionally, its height is subtracted from the
transcript, its right-hand side is reserved for a named escape gesture, and a
latched turn failure outranks every live phase and clears only when a turn starts.
Any queue indicator must compose with that precedence rather than replace it.

**The dialog conventions exist and are actively converging.** Panel chrome,
keymap hints, and geometry are being unified by the panel-convergence workstream,
whose M004 plan owns the shared chrome/geometry helper. There is deliberately no
shared list-selection helper today: vim navigation keys are duplicated per widget,
and the only shared paging primitive is the centered scroll helper. There are two
inherited rules a new box must obey: no box may advertise `Esc` or `Enter` as a
close verb, and a caller-supplied footer is not replayed into the keymap hint.

**Bare-letter keys in list contexts are largely claimed.** `d`, `e`, and `a` are
already bound by existing list widgets, `i` is the generic panel action key, and
the host intercepts panel-direction chords — including shifted `H`/`J`/`K`/`L` —
before a docked widget sees them, which is why one existing panel's advertised
reorder keys were dead. Bare `f` is unbound. A new box inherits the `d`/`e`/`a`/`f`
selection only if it binds them itself and advertises them; a modal box avoids the
host interception that made a docked panel's reorder keys unreachable.

**Desktop has an independent submit path.** The desktop client composes its own
turn submit and its TypeScript layer does not speak the core protocol. A queue UI
there is not inherited from the TUI.

## 5. Target architecture

A session-scoped, daemon-owned queue of user-authored prompts, each with a bounded
text body, a daemon-owned order, an optional delayed-delivery time, a status, and
an expiry. Frontends render a projection of it and issue explicit typed commands.

**Storage.** One additive migration introduces a queue-entry table: identity,
session, ordinal position, bounded text, optional delayed-delivery timestamp, status
(`pending`, `delivering`, `delivered`, `failed`, `expired`), an optional reason, and
expiry. Ordinal position is maintained by the daemon under a transaction so reorder
is a total order rather than a last-writer-wins patch.

**Protocol.** Typed queue operations as new request/response variants: list, add,
edit, delete, reorder, and request-now. All are additive on the wire. Each is
authorized by the controller predicate for mutation and session read authority for
listing. Queue reads ride the existing projection/seeding chain rather than
inventing a second live-delivery path.

**Delivery.** The queue never executes a prompt itself. On session availability,
the daemon selects the front due entry and submits it through ordinary
`TurnSubmit`, inheriting the controller lease, receipt, promotion consumption, and
no-replay rules. An entry that cannot be admitted stays pending or becomes visibly
failed. Mid-turn injection is the separate `TurnSteer` path with the honest
best-effort contract of ADR-0036.

**Delayed delivery.** Entries carry an optional due time, and `run_at_ms` gates
*eligibility* while `ordinal` orders among the entries already eligible — so reordering
cannot invert timer precedence.

> **Corrected 2026-10-18 (M007 planning).** This section previously read
> "Advancement is evaluated at an existing periodic observation point rather than by a
> new timer thread, so a `1 hr` delay introduces no new wakeup source". **That
> observation point does not exist.** Measured: the daemon's only periodic loops are
> projection-replay retention (300 s, conditionally spawned), opt-in provider refresh,
> and opt-in TTS, and M003's delivery entry has exactly two permitted callers — the
> post-terminal wake and the bounded restart twin. Neither fires for an idle session
> with a scheduled entry. Due-ness in the store query alone makes a due entry deliver
> **late, or not at all**, so M007 needs a named wakeup owner, and that is recorded as
> an open decision in `plans/implementation/user-prompt-queue/007-delayed-queue-delivery.md`
> §2 rather than assumed away here.
>
> **Updated 2026-10-18 (M007 implementation).** The §2 decision was taken: **Option A,
> a bounded queue-scoped advance tick**, recorded in ADR-0039 and implemented. M007's
> original exit condition — "advancement introduces no new timer thread or wakeup
> source" — **cannot stand as written**, because meeting the milestone's value requires
> exactly one new wakeup source. What the implementation does guarantee is narrower and
> is what that condition should have said: the wakeup **parks with no timer armed** when
> no session has a scheduled entry, and the tick's only work is to nominate sessions
> holding a due entry. Delivery authority is unchanged (ADR-0038).

**Frontend.** A `/queue` dialog that lists entries, supports keyboard selection and
scrolling, and performs delete, edit-in-place, add (with optional timer), reorder,
and send-now. It consumes the shared chrome/keymap seams. Working strip row 0
appends a queue-count suffix with a pointer to `/queue`, composed under the
established precedence.

## 6. Dependency graph

```text
ADR-0035 accepted
        |
        v
M001  contention refusal correctness          (capability)   [closed]
        |
        v
M002a durable queue storage + read projection (infrastructure) [closed]
        |                                    \
        v                                     v
M002b queue mutation ops + authority      ADR-0037 accepted
  (infrastructure) [closed]                 (Option B)
        |
        v
M003  queue delivery into turn admission       (capability)   [closed]
        v
M004  /queue box: list, select, delete, row 0  (capability)   [closed]
        |
        v
M005  /queue box: add, edit, reorder           (capability)   [closed]
        |
        +--> M006  steer frontend + send-now     (capability)   [closed]
        |
        +--> M007  delayed delivery (timer)      (capability)   [closed]
        |
        `--> C001  release-order displacement     (invariant)    [closed]
                  corrective; independent of
                  M006/M007; no migration

M008  desktop queue parity                       (capability)   [proposed, deferrable]
        depends on M004 + M005 (interface)
```

| From | To | Class | Note |
|---|---|---|---|
| ADR-0036 | M001 | hard (historical) | M001 closed borrowing only the refusal-taxonomy naming boundary; no ADR-0036 content was implemented. The ADR was `proposed` at the time and was **accepted 2026-10-18** |
| ADR-0035 | M002a | hard | Discharged. Accepted 2026-10-18; owns the durable entry model, bounds, and read authority |
| ADR-0037 | M002b | hard | **Discharged.** Accepted 2026-10-18 selecting Option B; owns the durable session-scoped queue-owner binding, its adoption/transfer operations, and the idle-mutation answer |
| M001 | M002a | hard | Discharged by M001 closure |
| M002a | M002b | hard | M002b builds on the shipped table, bounds, and ordinal constraint |
| M002b | M003 | hard | **Discharged by M002b closure.** Delivery requires authorized, durable, ordered entries |
| ADR-0038 | M003 | hard | **Discharged.** Accepted 2026-10-18 selecting Option B; owns the named delivery entry, attribution resolved from the owner row plus team state, re-authorization at delivery time, and at-most-once inherited from the receipt. Takes no migration number |
| ADR-0036 | M006 | hard | **Discharged.** Accepted 2026-10-18 selecting Option A; fixes steering as *acceptance into the turn's channel*, not delivery, forbids "force" in copy, retains a failed steer in its original position, and keeps the existing controller predicate. M004 closure **discharged**, so M006 was implementable |
| ADR-0036 | M007 | hard | **Discharged** 2026-10-18 for the "send now" copy boundary |
| M003 | M004 | hard | **Discharged** by M003 closure. A box over undelivered entries is not a capability |
| M004 | M005 | hard | Add/edit/reorder need the list and selection contract |
| M004 | M006 | interface | **Discharged by M006 closure.** Send-now needed entry identity and the retained-on-failure contract; the shipped request carries `entry_id` and **no prompt text and no `turn_id`** — the daemon loads the stored text and resolves the turn itself |
| M004 | M007 | interface | Timers need list rendering and ordering |
| panel-convergence (unowned) | M004 | interface | **Corrected 2026-10-18.** The named `panel-convergence M004` does not exist — zero occurrences in that addendum, and its M003 explicitly deferred the helper out as "its own milestone", which was never opened. The **geometry** half already landed (`centered_rect`, `src/tui/components/component.rs:290`). M004 duplicates the established per-widget `Chrome::{Owned, Hosted}` shape rather than forking a helper with no settled shape; it must not open that milestone itself |
| M004 + M005 | M008 | interface | Desktop parity consumes the box contract, not the TUI widget |

## 7. Milestones

### Milestone 1 — Turn contention refusal correctness

Class: capability

Objective: Make a submit refused because a turn is already active leave the
frontend's turn state truthful and the user's composed text intact.

Dependencies: ADR-0036 **proposed** at the time (later **accepted** 2026-10-18; its
failure taxonomy is used to classify the
refusal). Nothing else. This milestone is intentionally independent of the queue
so that the live text-loss defect can be fixed without waiting for the storage
design.

Deliverable boundary: refusal classification and per-class frontend response;
composed-text restoration for recoverable refusals; the shared request-id
completion guard applied to the submit path; active-turn state preserved when a
turn is still running.

User or operator value: a prompt typed during a turn is no longer destroyed, and
the UI no longer claims no turn is running while one is.

Exit conditions: a contention refusal leaves `active_turn_id`, `working_since`,
and session status consistent with the daemon; the composed text is recoverable;
a stale completion for a superseded submission cannot mutate current state; a
transport failure distinct from a refusal is not reported as contention.

Deferred work: all queue behavior.

### Milestone 2a — Durable queue storage and read projection

Class: **infrastructure**

Objective: Persist queued prompts with a bounded, expiring, daemon-ordered model
and expose a typed, read-authorized core request that returns them.

Plan: `plans/implementation/user-prompt-queue/002a-durable-queue-storage-and-read-projection.md`

Dependencies: M001 closed. ADR-0035 accepted (discharged 2026-10-18).

Deliverable boundary: one additive **v80** migration and the storage-layout bump;
the entry model with per-entry length, entry count, and expiry bounds; a
daemon-maintained ordinal with a schema-enforced total order; lazy deterministic
expiry; `UserPromptQueueList` under session **read** authority; the queue
projection on the frontend.

User or operator value: **none yet.** This milestone is machinery. It must not be
described as a user-visible queue, and its closure record must say in those words
that no user can queue a prompt. The read has no producer until M002b, so the
projection will always read empty — that is expected and must be stated.

Exit conditions: migration and layout-version assertions pass; bounds are
enforced as typed refusals **and** as schema `CHECK` backstops; expiry is
evaluated lazily, deterministically, and idempotently; a storage round-trip
survives reopening the pool; the read is refused without read authority; and no
`CoreRequest` variant can mutate the queue.

Deferred work: every mutation (M002b), delivery into turns (M003), and every
user-facing surface (M004+).

### Milestone 2b — Queue mutation operations and mutation authority

Class: **infrastructure**

Objective: Add the authorized add, edit, delete, and reorder operations under a
decided, durable mutation authority.

Plan: `plans/implementation/user-prompt-queue/002b-queue-mutation-operations-and-authority.md`

Dependencies: M002a closed. **ADR-0037 accepted** (discharged 2026-10-18, Option B).

Why the milestone was split: accepting ADR-0035 discharged the recorded blocker,
and the acceptance audit immediately exposed that ADR-0035's authority clause —
"the same controller predicate that governs turn admission" — has **no answer
while a session is idle**, because the durable lease is deleted at turn end and
the only idle-passing path is a session-unbound identity bypass. Mutating a queue
between turns is the primary use, so the authority model is a canonical
ownership-boundary decision that required its own ADR. Splitting kept the
dependency-clean storage work moving under every ADR outcome while the mutation
work waited for a maintainer decision.

Deliverable boundary: the four typed mutations (add, edit, delete, reorder); the
**v81** additive migration carrying the durable queue-owner binding; adoption and
revision-fenced transfer, both audited; the single generalised controller predicate
with a **named** call shape (`ControllerScope::Turn` vs
`ControllerScope::QueueMutation`) rather than an optional turn binding, because the
two questions must be distinguishable at every call site; audit coverage for every
mutating operation mapping to `prompt_submit`; `INSTRUMENTED_OPERATIONS`
classification.

**Outcome — closed** (`plans/closure/user-prompt-queue/002b-status.md`). v81
landed carrying only `user_prompt_queue_owner`; the four mutations reached
production producers; and `is_controller_bypass` gained no queue authority — its
early return is guarded by `matches!(scope, ControllerScope::Turn { .. })`, proven
by a test rather than by inspection. **The milestone delivered no user-visible
capability**: no command, dialog, key, row-0 change, or projection field, so
nothing a person can press reaches the six operations. The adoption policy
ADR-0037 deferred here is settled and audited — **the calling principal becomes the
owner** on first add, not the session creator and not the local interactive owner.

Two deviations from the plan's own text are recorded in the closure record rather
than smoothed over. `ControllerScope` shipped with **three** variants, not the two
the plan drew: routing `adopt` through `QueueMutation` would refuse it with
`QueueUnowned` and make ADR-0037's mandated repair path unreachable, so
`QueueAdoption` asks the question adoption actually asks. That was found by a
failing test, and the **code** was wrong rather than the test. Two real bugs were
also caught by the new tests and fixed before closure: `delete` read its surviving
projection without renumbering them, leaving a permanent ordinal gap, and
`is_permutation` had an inverted `HashSet` duplicate check — `!insert` accepts a
duplicate rather than rejecting it, because `insert` returns `true` when the value
is *new*.

User or operator value: **none yet.** Still no user-visible surface; that is M004.

Exit conditions: a queued entry can be added, edited, deleted, and reordered
**while idle** with no lease row; every mutation is refused for an unauthorized
principal in both the idle and mid-turn states with a typed reason; all four
owner/controller mismatch combinations mid-turn are typed refusals rather than a
precedence rule; reorder is a dense total order and concurrent mutations converge;
edit changes neither identity nor position; ownership survives restart; concurrent
transfers converge on one winner; and `TurnCancel`/`TurnSteer` authorization tests
pass unmodified.

Deferred work: delivery (M003) and every user-facing surface (M004+).

### Milestone 3 — Queue delivery through ordinary turn admission

Class: capability

Objective: Make a due queue entry become a real turn through the existing
admission path, with at-most-once delivery and no replay.

Dependencies: M002b closed. ADR-0035 accepted. ADR-0038 **accepted** 2026-10-18
selecting **Option B**.

Plan: `plans/implementation/user-prompt-queue/003-queue-delivery-through-turn-admission.md`
(**closed** — `plans/closure/user-prompt-queue/003-status.md`)

Inherits one bounded decision from M002a: the v80 `status` `CHECK` enumerates
all five roadmap values so M003 does not have to rebuild a durable user table,
which means `delivering`, `delivered`, and `failed` are unreachable until this
milestone. M003 must either make all five reachable or rebuild the table, and
must say which. **Answered in the plan:** make all five reachable. `delivering`
on claim, `delivered` on a recorded turn, `failed` on a recorded refusal. v80 is
not edited and no rebuild is performed.

Inherits a second, larger decision that M002a and M002b both left open — **as
whom** is a queued entry submitted? The daemon may originate an admission, but
the repository's only such path (`submit_idle_continuation_turn`, ADR-0031)
reconstructs authority from durable attribution via
`AuthenticatedPrincipal::reconstructed`, which needs five fields, and the v81
owner row stores one (`owner_principal`). Delivering under a synthetic daemon
identity would take the turn controller lease away from the real owner and lock
the owner out of its own queue at its first mid-turn mutation.
`plans/adrs/ADR-0038-queue-delivery-execution-authority.md` **resolved** this on
2026-10-18 by accepting **Option B**: a second, separately guarded named entry
(`submit_queued_prompt_turn`), attribution reconstructed from the owner row plus
`TeamStore`, `AuthMethod`/`TransportClass` bound to a daemon-owned delivery
classification rather than to a dead connection, and re-authorization against
current team state at delivery time.

**The v82 contest is therefore resolved in M007's favour.** Option B needs no
migration, so `run_at_ms` stays **v82** and M003 takes no migration number at
all.

Deliverable boundary: due-entry selection; submission through ordinary
`TurnSubmit`; status transitions; refusal and failure handling that retains or
visibly fails an entry; observation/audit of delivery.

User or operator value: a prompt composed during a turn is delivered as the next
turn automatically.

Exit conditions: delivery is at most once across reconnect and daemon restart; an
entry interrupted mid-admission is reconciled by receipt status and never
double-submitted; a refusal retains the entry rather than dropping it; ordering is
respected; concurrent frontends cannot double-deliver one entry.

Deferred work: mid-turn injection, and all user-facing management surfaces.

### Milestone 4 — `/queue` box: list, select, delete, and status indicator

Class: capability

Objective: Give the user a visible, navigable list of queued prompts and an
always-readable indicator that prompts are waiting.

Plan: `plans/implementation/user-prompt-queue/004-queue-box-list-select-delete-and-indicator.md`
(**closed 2026-10-18**; closure record
`plans/closure/user-prompt-queue/004-status.md`)

Dependencies: M003 closed (**discharged 2026-10-18**). ADR-0035 accepted.
ADR-0036 accepted (constrains vocabulary only; does not gate this milestone).
Interface dependency on the panel-convergence shared chrome helper — **measured
and found not to block**, see below.

**The named interface dependency does not exist, and its absence does not
block.** §6 and §10 named `panel-convergence M004` as the chrome/geometry
helper's owner. Measured against that addendum: `M004` has **zero occurrences**
in it — it defines only M001/M002/M003, and M003's own text *defers* the helper
out ("it has no seam, no failing test to define it… the helper should be its own
milestone"). No such milestone was ever opened. The **geometry** half already
landed and is de-duplicated (`centered_rect`, `src/tui/components/component.rs:290`);
only the **chrome** half is missing, and each widget already declares its own
module-private `enum Chrome { Owned, Hosted }`. This milestone therefore takes
the roadmap's own fallback — **duplicate the established chrome rather than fork
a helper with no settled shape** — and M004 is `ready`.

**The dialog shape is corrected against source.** The legacy per-variant key
router is `#[cfg(any())]`-disabled (`src/tui/app/mod.rs:4364-4368`); FocusManager
is the sole key authority. The box must be a FocusManager-mounted `Component`,
and only the two `From` impls between `Dialog` and `DialogType` are exhaustive
for a new variant. An arm added to the dead router would compile and do nothing.

**Row 0's shape is corrected against source.** §4 described row 0's "right-hand
side reserved for a named escape gesture"; in code `stop_hint`
(`src/tui/app/render.rs:1690-1703`) is a **left-aligned `" · "`-joined suffix**
on the same line. The queue suffix composes with that shape.

**Delivered 2026-10-18.** The exit conditions below all hold. Three things were
established during implementation that the planning did not say:

- **The box is a `Component`, and `Dialog::Queue` has no key arms.** Only
  `QueueDialog::handle_key` implements keys. `open_dialog` got an explicit
  no-op arm so the no-op is declared rather than reached by falling through, and
  `dock_target` still has no queue arm so the box stays a modal.
- **Selection follows `entry_id`, not the index.** A projection that loses a row
  — deleted, delivered, or expired — would otherwise slide the cursor onto a
  different prompt, and the `d` pressed next would remove something the user
  never looked at. When the selected entry is gone the cursor clamps.
- **One applier serves all three producers.** The read on open, the `r` refresh,
  the M003 push, and the delete response all converge on
  `apply_user_prompt_queue_loaded`, which now also updates the mounted box
  through `focus_manager.with_dialog_mut`. Row 0's count and the open list
  therefore cannot disagree.

Two guards were proved non-vacuous by injecting a violation, and one of them
needed two corrections before it measured anything — see the closure record §4.
The "no second read on delete" guard first counted transport calls immediately
after `on_key` returned, which races the `spawn_tui_task` worker; and after that
was fixed it still passed against an injected second read, because everything
the test drove ended at the *request* and the apply path was never executed at
all. That hole is now closed and the injected read fails the test.

Deliverable boundary: `/queue` command registration and dispatch; the
`Dialog::Queue`/`DialogType::Queue` identity and a FocusManager-owned list
component with selection and scrolling; a read producer on open; delete of the
selected entry applying the projection the delete itself returns; working strip
row 0 queue suffix.

User or operator value: the user can see that prompts are waiting, open the queue,
move through it, and discard one they no longer want. **This is the workstream's
first user-operable milestone.**

Exit conditions: the indicator appears only when entries exist and composes under
the established row 0 precedence without adding a row or reflowing the transcript;
selection works with both arrow and vim keys; delete removes exactly the selected
entry and is not requeued on close; every advertised key works **through the real
key path**, and the dialog advertises nothing it does not do; an observer cannot
open or mutate it; the empty state names nothing M005 adds.

Deferred work: adding, editing, reordering (M005), send-now (M006), and timers
(M007).

### Milestone 5 — `/queue` box: add, edit in place, and reorder

Class: capability

Objective: Let the user author, revise, and order queued prompts.

Plan: `plans/implementation/user-prompt-queue/005-queue-box-add-edit-and-reorder.md`
(**closed** — closure **discharged 2026-10-18**; see
`plans/closure/user-prompt-queue/005-status.md`)

Dependencies: M004 closed. ADR-0037 accepted (adoption policy is settled: the
calling principal becomes the owner on first add). ADR-0036 constrains copy only.

**Three store facts, measured in M003's `user_prompt_queue.rs`, decide whether
this is a frontend milestone or a store correction.**

1. **Reorder's permutation is checked against the PENDING set only**
   (`reorder` → `list_live_in` → `OPERABLE_STATUS_PREDICATE`,
   `crates/codegg-core/src/session/user_prompt_queue.rs:839-865`, `:1157`), while
   the client projection carries pending **and** delivering
   (`list_projected_in`, `:1184`). A TUI that sends every id it rendered is
   refused with `OrderMismatch` the moment any entry is delivering.
2. **`renumber_dense` displaces every row in the session, status-blind**, before
   densifying the targets. The band offset prevents an `UNIQUE (session_id,
   ordinal)` collision — but a `delivering` row's ordinal jumps above every
   pending row, so it renders last, and if its delivery is later **released** it
   returns to `pending` at the end of the queue. Closed M002b/M003 behaviour;
   M005 must test it and record the answer rather than work around it.
   **Measured 2026-10-18 (M005):** confirmed, and sharper than anticipated —
   the released entry keeps the displacement ordinal
   (`USER_PROMPT_QUEUE_RENUMBER_BAND_OFFSET`, 1 000 000) rather than being
   re-densified, so it sits outside the dense band entirely. Recorded as
   accepted-and-documented behaviour; changing `renumber_dense` reopens closed
   M002b/M003 and needs a maintainer decision.
   **Annotated 2026-10-18 (M006/M007 planning):** the maintainer decision has
   since been made, and planning it sharpened the severity beyond what M005
   recorded. `claim_next_for_delivery` selects `ORDER BY ordinal ASC`, so the
   returned entry is not merely **rendered** last — it is **delivered** last —
   and all **three** return paths are affected, not just `release_delivery`:
   `retry_failed` and `resolve_stranded_delivery(Retry)` also restore `pending`
   with no ordinal write. That contradicts ADR-0035's own "a trigger that fires
   out of order must not reorder the user's work", so it is a defect against
   accepted architecture rather than a design choice. Resolved by
   `plans/implementation/user-prompt-queue/C001-released-delivery-order-displacement-corrective.md`,
   which fixes it **without a migration** by renumbering the whole live set and
   keeping non-pending rows at their existing rank. M005's closure record is
   **not** rewritten; the two tests that asserted the defect are replaced by
   position-preservation tests, and the `docs/tui.md` workaround sentence
   ("add a prompt last rather than reorder an older one") is removed.
   **Annotated 2026-10-18 (C001 closure):** closed. `merge_operable_into_live`
   now folds the client's pending order with each non-pending live row held at
   **the rank it already had**, and `renumber_dense` writes that merged live set.
   The displacement step is unchanged and terminal rows are still displaced — that
   is what keeps `UNIQUE (session_id, ordinal)` safe. A released or reconciled
   entry is now **order-neutral**: it kept a dense ordinal while stranded, so
   restoring `pending` moves neither it nor its neighbours. No migration;
   `STORAGE_LAYOUT_VERSION` stays 81 and v82 stays M007's. Two findings were
   reported rather than fixed, per C001 §14 — see §10.
3. **`entry_id` is client-supplied**, so the TUI mints it — which is what makes
   add idempotent under retry.

Deliverable boundary: add-entry input that queues immediately and never
dispatches; edit-in-place text replacement that changes neither identity nor
position; reorder of the selected entry, with the permutation built from pending
rows only; an `is_text_step()` predicate guarding every action key, following
the `Connect` dialog precedent (`src/tui/components/dialogs/connect.rs:495-501`,
`:1189-1195`) because no shared `captures_text` predicate exists.

**As delivered.** The guard is **structural**: `handle_key` matches the text
step first as a separate match and returns from it, so the browsing arm is
unreachable while a composer is open — a later sixth action cannot be live there
by forgetting a guard. Reorder keys are `[`/`]`, not `Shift+K`/`Shift+J`:
`docks_as_viewer()` defaults to `false`, so the plan's reasoning about shifted
keys was a **wrong premise** for this box, which overrides it to `true`. The
keys survive because `App::dock_target` has **no** `DialogType::Queue` arm, so
the whole docking-steal branch is dead regardless of the viewer flag; that is
asserted, not assumed. One local refusal exists and is the deliberate exception
to M004's "let the daemon answer": a `delivering` selection cannot be reordered
daemon-side at all — the permutation would omit the row, **succeed**, and
reorder nothing.

User or operator value: the queue is authorable and correctable rather than
append-only.

Exit conditions: add defaults to queueing immediately rather than dispatch; edit
replaces text without changing position or identity; reorder is a total order and
survives restart; **reorder succeeds with a `delivering` row present**; typing
`d`/`e`/`a`/`j`/`k` inside a text step produces text and never an action; the
keymap hint names no key the host answers first; closing the dialog does not add,
edit, or requeue what was just abandoned.

Deferred work: timers and send-now. Adoption and transfer stay explicit, audited
operations outside the box — ADR-0037 requires it.

### Milestone 6 — Send-now steering from the queue

Class: capability

Objective: Deliver the selected entry into the running turn now, honestly.

Plan: `plans/implementation/user-prompt-queue/006-send-now-steering-from-the-queue.md`
(**closed** — every dependency discharged). Closure record:
`plans/closure/user-prompt-queue/006-status.md`

Dependencies: M004 closed (interface). **ADR-0036 accepted** 2026-10-18 —
**discharged**. M005 closed (interface).

**ADR-0036 names this plan path explicitly**, which is why it is the one milestone in
this roadmap whose authorization is a precondition rather than a coincidence of timing.

**The decision ADR-0036 deferred to this milestone is made here:** on `Ack` the entry is
marked `delivered`, bound to the accepting turn, rather than left `pending` or deleted.
Leaving it `pending` would run the same instruction twice — once steered, then again
claimed by M003 as a fresh turn — which is precisely the duplication ADR-0036's decision
drivers forbid. The schema already carries `delivered_turn_id` and `delivered_at_ms`, so
this needs **no migration**, no new `status` value, and no table rebuild; the only new
store work is one `pending → delivered` transition guarded on `pending`.

**Steering does not consult the queue owner.** ADR-0037's exclusion is quoted verbatim
in the plan: steering addresses a *running* turn, which has a live controller lease,
while ADR-0037 governs mutation *between* turns. M006 routes send-now through the
existing turn-controller predicate and must not widen it.

Deliverable boundary: transport-authority-correct dispatch of the existing
`TurnSteer`; retained-on-failure semantics with a user-visible reason; distinct
presentation for authorization refusal, channel-full, receiver-closed, and
unsupported.

**Delivered 2026-10-18.** `s` ships end to end: `UserPromptQueueSendNow`
request/response, `handle_user_prompt_queue_send_now`, `mark_steer_delivered`,
and `DialogState::queue_send_request` on its own generation. Four facts worth
carrying forward:

- **The request carries `entry_id` and nothing else** — no prompt text, no
  `turn_id`. The daemon loads the stored text (ADR-0035 makes that safe) and
  resolves the active turn from its own runtime handle, so a client cannot
  submit unattributed text or steer a turn that has already ended.
- **The handover is gated by the turn controller, never by
  `ControllerScope::QueueMutation`.** The queue-ownership question is re-asked
  *inside* `mark_steer_delivered`'s transaction, where it is about a write to
  the queue rather than a reach into a turn.
- **`steer_channel_code` is now shared with `TurnSteer`**, so direct steering and
  send-now cannot disagree about what a full or closed channel means.
- **One outcome the plan's two-valued shape did not have**:
  `AcceptedNotSettled`, for the case where the turn took the prompt and the
  settlement failed. Reporting it as an error would tell the user to resend a
  prompt that already ran.

User or operator value: a correction or redirection reaches the agent without
waiting for the turn to end.

Exit conditions: all three previously unreachable backend error branches have
product-visible behavior; a failed send retains the entry and its position;
copy never claims forcing or guarantee; the transport is selected by attach
transport rather than by app mode; an observer is refused distinctly from a
channel condition.

Deferred work: nothing within this milestone; capacity and fairness policy on the
agent-loop side remain open. True at-most-once across a *lost* `Ack` needs a durable
steer-attempt record and is explicitly out of scope — the plan reports the unknown
outcome instead of retrying.

### Milestone 7 — Delayed queue delivery

Class: capability

Objective: Allow an entry to be scheduled for a future time, such as one hour out.

Status: **implemented and closed** 2026-10-18 — `plans/closure/user-prompt-queue/007-status.md`.
The §2 decision was taken as **Option A** (bounded queue-scoped advance tick) and is
recorded in `plans/adrs/ADR-0039-queue-advancement-wakeup-authority.md`, which is
**proposed, not accepted** — acceptance is a maintainer action.

Plan: `plans/implementation/user-prompt-queue/007-delayed-queue-delivery.md`

Dependencies: M004 closed (interface). M005 closed (interface). ADR-0036's "send now"
copy boundary — **discharged** 2026-10-18.

**The roadmap's own basis for this milestone was falsified while planning it.** It
claimed advancement rides "an existing periodic observation point". No such point exists:
the daemon's only periodic loops are projection-replay retention (300 s, conditionally
spawned), opt-in provider refresh, and opt-in TTS, and M003's delivery entry has exactly
two permitted callers — the post-terminal wake and the bounded restart twin. Neither
fires for an idle session holding a scheduled entry, so due-ness enforced only in the
store query makes a due entry deliver **late, or not at all**.

**Resolved 2026-10-18: Option A.** One daemon task computes the earliest due instant
across all sessions, sleeps exactly until it, and advances every session holding a due
entry. With nothing scheduled it **parks on a `Notify` with no timer armed**, which is
what keeps an idle daemon genuinely idle; a successful `add`/`edit` signals that
`Notify`, so a newly scheduled entry can wake a parked task. This widens
`PERMITTED_CALLERS` in `scripts/check_queue_delivery_callsites.py` from two to three —
a recorded decision, not a quiet third call site — and the guard's `--self-test` now
injects a **fourth** caller and proves it is still rejected. See ADR-0039.

**Reorder-versus-timer precedence is decided here**, as ADR-0035 required: `run_at_ms`
gates *eligibility* and `ordinal` orders among the entries already eligible. A reorder
therefore cannot invert timer precedence, because a not-yet-due entry is invisible to the
ordering. This takes the roadmap's second offered branch — the ordering rule is explicit
and tested — and does not degrade M005's reorder.

**A duration parser already exists and must be reused:** `parse_duration`
(`src/background_task_migration.rs:11-34`) accepts `30s`, `5min`, `10m`, `2h`, `1d`, and
returns `None` for anything else — including a **bare number**, which must be refused
rather than guessed, and **`1 hr`**, which the roadmap used as its example and which
does not parse. Copy must say `1h`.

**Expiry bounds a due time.** An entry that becomes due at or after its own expiry could
never run, so that combination is refused at add and edit — a typed refusal, never a
silent clamp.

Deliverable boundary: optional due time on an entry, captured at add time and
editable; parsing and display of a human duration; advancement at a bounded,
parking, queue-scoped wakeup; explicit interaction between reordering and timers.
`run_at_ms` is added as a nullable column in **v82** (in-place
`ALTER TABLE … ADD COLUMN`), because v80 and v81 are already spent — and **no**
`CHECK`, because a nullable column with no sentinel admits every value a writer can
produce. v80's `status` `CHECK` could not have been altered at all, so a
"scheduled" status was never available additively.

User or operator value: a user can leave an instruction for later without holding
the session open.

Exit conditions: a due entry is not delivered early; **the wakeup parks with no
timer armed when nothing is scheduled** (corrected 2026-10-18 — see §5); an
expired-by-time entry expires with a recorded reason; reordering cannot silently
invert timer precedence — either reorder is refused for scheduled entries or the
ordering rule is explicit and tested; a daemon restart does not lose or
prematurely deliver a due time.

**All met**, with two recorded deviations and one defect found and fixed:

- **Deviation (copy/UX):** the authoring surface is a **second step**, not an
  inline `| 1h` suffix. Any inline delimiter is a character the user cannot type,
  so it would reinterpret prompt text. `a`/`e` advance from text to duration.
- **Deviation (delivery):** a due time is a **floor, not a promise** — delivered at
  the first turn boundary at or after the instant, and *late* if the daemon was
  down. Copy asserts no wall-clock precision.
- **Defect found by the wakeup tests:** a due-but-undeliverable entry (session
  busy, owner gone, composition refused) stays `pending` and permanently due, so
  the tick re-armed at zero delay and spun. Fixed with `SAME_DUE_RETRY_FLOOR_MS`,
  a damper on retrying *the same* due instant — not a poll interval, and it does
  not delay the post-terminal wake or the restart twin.

Deferred work: recurring schedules and calendar-style triggers.

### Milestone 8 — Desktop queue parity

Class: capability

Objective: Bring the queue to the desktop frontend.

Dependencies: M004 and M005 closed (interface).

Deliverable boundary: desktop-side queue reads and the mutating operations, using
the same protocol variants and the same authority rules.

User or operator value: the capability is not silently absent on a supported
frontend.

Exit conditions: desktop lists, adds, edits, deletes, and reorders against the
shared protocol; parity is verified or the absence is explicitly documented and
accepted as a known gap; no second queue model is introduced in the desktop layer.

Deferred work: graphical/IDE queue presentation beyond parity.

## 8. Cross-cutting requirements

### Storage and migration

Two additive migrations in sequence: **v80** for the queue entry table (M002a) and
**v81** for the queue-owner binding (M002b). The layout constant must be bumped to
match at each step and both existing assertions must pass. A shipped migration is
never edited. Entry count, per-entry text length, and expiry are
part of the contract and exceedances are typed refusals, never silent truncation.
Order is a daemon-maintained ordinal under transaction. The release-order
corrective C001 took **no** migration at all and is closed, so
`STORAGE_LAYOUT_VERSION` remains **81**. A third additive migration, **v82**,
carrying `run_at_ms`, is still reserved and unspent for M007.

### Protocol and compatibility

All additions are new variants; no existing variant changes shape. A frontend
that predates the queue reads an absent projection as an empty queue. No queue
operation may reuse a request whose semantics differ, and `TurnSteer` is used
unchanged.

### Security and authorization

Queue mutation is authorized by the durable, session-scoped **queue owner**
binding that ADR-0037 accepted, and — mid-turn — additionally by the turn-controller
lease, with a mismatch refused rather than resolved by precedence. It does **not**
reuse the turn-controller predicate alone, because that predicate does not exist
while idle; queue reads respect session read authority. Observers are refused for
every mutating operation and must be
registered as blocked for the `/queue` command and its dialog. Prompt text is
never logged verbatim and never echoed into receipts or error messages. The
product's one-time 1 MiB turn text bound does not license an unbounded queue
entry.

### Concurrency, cancellation, and recovery

One entry is delivered at most once. Concurrent frontends contend through the
lease. Canceling a turn does not discard the queue. Daemon restart reconciles
mid-admission entries by receipt status and never replays. Reconnect re-reads the
queue as authoritative rather than replaying a local copy.

### Observability and audit

Queue mutation is auditable in the same shape as other authorized operations,
with entry identity rather than text. Delivery outcomes are observable. Neither
diagnostics nor audit records contain prompt bodies.

### Performance and resource use

Advancement is evaluated at an existing observation point; no new timer thread or
global scheduler. Bounded entry count and text length prevent unbounded growth.
Rendering is bounded per row with announced truncation.

### Documentation and operations

`architecture/tui.md` and the `tui` skill gain the box, the keys, and the row 0
suffix. `architecture/core.md` and the `core` skill gain the queue ownership and
delivery contract. `architecture/session.md` gains the storage model. `docs/tui.md`
gains user-facing guidance. The two steering mechanisms are cross-referenced
wherever either is described, and the accepted ADR-0007 relationship is stated so
the queue is not mistaken for a second control authority.

## 9. Verification strategy

- Migration tests asserting v79 pools migrate to v80 and the layout constant
  matches the highest migration.
- Storage round-trip and atomicity tests for every operation, including reorder as
  a total order.
- Contention tests: two principals racing one queue; two controllers racing
  delivery; concurrent reorder attempts.
- Restart and recovery tests: frontend reconnect, frontend restart, daemon restart,
  and an entry interrupted mid-admission.
- Authorization tests for every mutating operation against an observer, a
  non-controller contributor, and an unrelated session.
- Transport tests proving the WS-attach and socket-daemon paths both work and that
  send-now routes by attach transport rather than app mode.
- Dialog tests driving keys through the real key dispatch path rather than by
  calling handlers directly, including Insert-mode safety and the established rule
  that a close must not requeue what it just acted on.
- Render tests proving the row 0 suffix composes under existing precedence,
  including while a latched error is displayed, and that no row height changes.
- Documentation and copy checks, including that steering copy avoids guaranteeing
  delivery.

## 10. Risks and decision points

| Risk or decision | Resolution |
|---|---|
| Queue ownership could drift to the frontend and lose durability on reconnect | ADR-0035 fixes daemon ownership; M002a and M002b are infrastructure and cannot be claimed as capability |
| The refusal defect is left in place while the queue is built, so the queue is designed on corrupt turn state | M001 is a hard predecessor of M002a |
| The controller lease does not exist between turns, so queue mutation has no authority while idle | **Discharged.** ADR-0037 accepted 2026-10-18 selecting Option B, a durable session-scoped queue-owner binding that exists while idle and survives restart; `is_controller_bypass` is explicitly not extended |
| Two ownership notions sit adjacent and get confused at a call site | ADR-0037 requires the predicate's call shape to name the question (`ControllerScope::Turn` vs `ControllerScope::QueueMutation`); `architecture/core.md` states which is which |
| An invalidated queue silently becomes owned by whoever touches it next | An `Absent` queue adopts atomically on first add, but an `Unowned` queue is refused for **every** mutation and repaired only by the explicit adoption operation |
| Steering copy promises more than the channel delivers | **Discharged.** ADR-0036 **accepted** 2026-10-18 selecting Option A; the guarantee is acceptance into the turn's steering channel, not delivery, and "force" is forbidden in copy. M006 carries the tests, each written *after* the frontend exists so it exercises the key path rather than the handler |
| The shared chrome helper has not landed, so the box forks chrome | **Corrected 2026-10-18.** The helper's owning milestone does not exist, and geometry has already landed de-duplicated. M004 duplicates the established per-widget chrome shape and explicitly does not open that milestone |
| Reorder's permutation is built from the projected set instead of the pending set, so it is refused with `OrderMismatch` as soon as any entry is delivering | **Identified 2026-10-18.** `reorder` checks against `OPERABLE_STATUS_PREDICATE` (`status = 'pending'`) while the projection carries pending **and** delivering. M005 must filter before permuting |
| A reordered `delivering` entry silently jumps to the end of the queue when its delivery is released | **Closed 2026-10-18 by C001.** M005 measured the render symptom; planning C001 measured the delivery symptom — `claim_next_for_delivery` orders by `ordinal`, so the entry is delivered last too — and found **three** affected return paths, not one. This contradicted ADR-0035's stated intent, so it was a defect against accepted architecture. Fixed with **no migration**: `merge_operable_into_live` folds the client's pending order with each non-pending live row held at **the rank it already had**, and `renumber_dense` writes that merged live set. `release_delivery` and `resolve_stranded_delivery(Retry)` are now order-neutral. Closure record `plans/closure/user-prompt-queue/C001-status.md` |
| `retry_failed` revives a row that was **terminal** during the reorder, so it is absent from the merge's live set and returns above the band | **Found and reported 2026-10-18 by C001, not fixed.** C001's §6 claims all three return paths are covered with no change to those functions; that claim holds for `release_delivery` and `resolve_stranded_delivery(Retry)` and **falsifies here**. The entry is not lost — it is delivered last. `retry_failed` has **no production caller** (the daemon's non-terminal path is `release_delivery`), and a `failed` entry is absent from the client projection, so no user ever watched it move. Where a revived `failed` entry *should* land — stale ordinal, head, or tail — is an **open maintainer decision**. Pinned by `user_prompt_queue_a_retried_failure_is_not_reached_by_the_c001_merge` |
| A uniform displacement is only collision-free while no row already sits at or above the band, so a **second** renumber after a displaced terminal row violates `UNIQUE (session_id, ordinal)` | **Found and reported 2026-10-18 by C001, not fixed.** Pre-existing and independent of the merge: the displacement `UPDATE` is byte-identical before and after C001 and collides inside itself. Minimal reproduction, reachable from the queue box alone — queue three prompts, reorder, let one fail, reorder, **reorder again**: the third is refused with a storage error. C001 §14 stops the milestone on exactly this ("a second displacement appears to be needed to keep `UNIQUE`; stop"), and a correct fix (a monotonic band generation, or compacting terminal rows) is a design decision, not an implementation detail. Pinned by the exhaustive enumeration, which counts **4** of 1 554 steps and asserts the first triggering row state exactly |
| M007's advancement has no wakeup: the "existing periodic observation point" it was scoped against does not exist | **Falsified 2026-10-18.** The daemon's only periodic loops are projection retention (300 s, conditionally spawned), opt-in provider refresh, and opt-in TTS; M003's delivery entry has exactly two permitted callers, neither of which fires for an idle session holding a scheduled entry. Due-ness in the store query alone delivers late or never. M007 was registered `blocked` on one named decision — the wakeup owner — and **resolved 2026-10-18 as Option A** (bounded queue-scoped advance tick), recorded in ADR-0039 (**proposed**; acceptance is a maintainer action). `PERMITTED_CALLERS` in `scripts/check_queue_delivery_callsites.py` went from two to three with the rationale on the constant, and `--self-test` now injects a **fourth** caller and proves it is still rejected |
| A bare or misspelled duration is silently guessed ("5" meaning five seconds, "1 hr" not parsing) | M007 reuses the repository's existing `parse_duration` grammar (`30s`, `5min`, `10m`, `2h`, `1d`) and refuses anything it does not accept. The roadmap's own `1 hr` example does not parse and must not appear in copy |
| A queued entry becomes due at or after its own expiry and could never run | M007 refuses that combination at add and edit as a typed refusal, never a silent clamp |
| Two concurrent queue write transactions can **both** fail with `SQLITE_BUSY` (code 6, `database is deadlocked`) instead of one winning | **Found by M006 while repeating a suite, not fixed.** Every queue write path is `begin()` (deferred) → `SELECT` → `UPDATE`, and SQLite refuses to upgrade a shared read lock to a write lock while another connection holds a read lock — without waiting, because waiting could deadlock, so `busy_timeout` does not retry it. M003's `user_prompt_queue_concurrent_claims_admit_exactly_one_entry` fails this way on **both** futures, reproduced **4 times in 12 runs**; it surfaced only because M006 does not trust a single green run. The **at-most-once fence is unaffected** — neither caller wrote, so nothing is double-delivered — and production exposure is bounded by ADR-0038's two permitted callers and M002b's single owner. The fix is a **store-wide** transaction-shape decision (`BEGIN IMMEDIATE` on write paths, or a single-statement fence) and belongs to a corrective milestone, not to a feature. M006's own concurrency test was hardened to assert *at most one* wins rather than *exactly one*, which is the contract the store actually has. Pinned by `user_prompt_queue_a_steer_settlement_is_at_most_once`; the M003 test is annotated, its assertion untouched |
| A steered entry left `pending` runs twice — once steered, then again as a fresh turn | **Closed 2026-10-18 by M006.** `mark_steer_delivered` moves `pending` → `delivered` and stamps `delivered_turn_id` with the turn that accepted it, so the queue cannot hand the same prompt out again. Guarded on `status = 'pending'`, so a row M003 already claimed is refused rather than settled twice. **No new `status` value and no migration**; `delivered_turn_id` already existed |
| The turn accepts a prompt but the queue cannot record the settlement, so the row stays `pending` and may run **again** | **Found by M006 and given its own outcome rather than folded into a refusal.** `AcceptedNotSettled` reports exactly this: the turn holds the text and the queue row did not move. Reporting it as an error would invite the user to resend a prompt that already ran — the duplication ADR-0036 forbids — so the TUI copy tells the user to treat it as sent and check before repeating. Audited as `accepted_unsettled`, which is the only record of a prompt the audit chain can show was delivered without a matching `turn_admission_receipt` |
| Host chord interception makes advertised keys dead, as happened to an existing docked panel's reorder keys | Modal dialog avoids the docked host; a non-vacuous test must prove every advertised key reaches its action |
| Reordering silently inverts timer precedence | **Resolved 2026-10-18 (M007).** `run_at_ms` gates *eligibility*; ordering stays `ORDER BY ordinal`. A not-yet-due entry is invisible to the ordering, so reorder permutes only entries already eligible and cannot promote a scheduled one. Pinned by `user_prompt_queue_reorder_cannot_deliver_a_not_yet_due_entry_early` and `user_prompt_queue_ordinal_orders_eligible_entries_and_skips_not_yet_due_ones` |
| The advance tick spins at full speed while a due entry stays undeliverable | **Found by M007's own wakeup test, and fixed.** A due entry that cannot be delivered — session busy, owner gone, composition refused — stays `pending` and permanently due, so the tick re-armed at `wait = 0` forever. `SAME_DUE_RETRY_FLOOR_MS` (30 s) damps retrying *the same* due instant. It is not a poll interval: the task still parks with no timer when nothing is scheduled, and the post-terminal wake and restart twin are untouched |
| A queue becomes unbounded disk growth | Count, length, and expiry bounds are contract, enforced as typed refusals |
| Two "queued prompt" mechanisms drift apart | ADR-0036 fixes terminology and requires cross-references |
| Desktop silently lacks the capability | M008 is a registered milestone, explicitly deferrable, and its deferral must be stated rather than implied |
| The queue becomes a second scheduler | ADR-0035 rejects the job-execution model explicitly; advancement rides existing observation |
| A queued entry is submitted under a principal that is not the queue owner, taking the controller lease from the owner and locking it out of its own queue at the first mid-turn mutation | ADR-0038 (**accepted**, Option B) owns the delivery authority: the delivered principal is the queue owner, resolved from durable rows and re-authorized against current team state; the M003 plan forbids a synthetic daemon principal and `is_controller_bypass`, and requires asserting the receipt's `principal_id` rather than inspecting the call path |
| The daemon's ability to submit as a principal widens into a backdoor | M003's delivery entry is named and restricted to two callers by a callsite guard in the shape of `scripts/check_idle_continuation_callsites.py`, and the guard must be proven against an injected third caller |
| A `delivering` row is stranded: invisible to the read projection, uncounted by the backlog bound, and never swept by expiry | M003 work package B treats startup reconciliation of `delivering` rows by receipt status as a correctness requirement; the three `status = 'pending'` literals are each called out explicitly |
| M003 and M007 both need migration v82 | **Resolved.** ADR-0038 selected Option B, which needs no migration; `run_at_ms` keeps v82 and M003 takes none. **Shipped 2026-10-18**: M007 owns v82, `STORAGE_LAYOUT_VERSION` is **82**, and v80/v81 are untouched |
| A delivered entry keeps showing in the frontend as waiting | The queue has no `CoreEvent` and reloads only on the session-change edge; M003 owns adding the push path plus a TUI arm, because a stale projection here is a correctness defect |

## 11. Completion definition

This roadmap may be closed only when all of the following are true:

- A prompt composed during an active turn is retained and later delivered, and the
  composed text is never destroyed by a refusal (M001, M003).
- Delivery is at most once across frontend reconnect, frontend restart, and daemon
  restart, proven rather than asserted.
- Queue order is daemon-owned and survives restart.
- Every mutating operation is authorized by the durable queue-owner binding and,
  mid-turn, by the turn controller as well; observers are refused for all of them.
- The user can list, select, add, edit, delete, and reorder entries, and can see
  that entries are waiting from the status line.
- Send-now reports its true best-effort guarantee and has product-visible behavior
  on every backend error branch. **Met 2026-10-18 by M006**: the guarantee is
  *acceptance into the turn's steering channel*, every outcome gets its own
  sentence, and no copy says "forced", "guaranteed", or "always reaches" — the
  last three are asserted absent by
  `user_prompt_queue_the_send_now_sentences_are_all_different`.
- Delayed delivery neither fires early nor loses its due time across restart, and
  its interaction with reordering is explicit. **Met 2026-10-18 by M007**: the
  eligibility predicate is one shared expression used by both the claim and the
  sweep, ordering stays `ORDER BY ordinal`, and a due time is a **floor** — late
  across a restart, never early. The wakeup parks with no timer armed when
  nothing is scheduled.
- Desktop parity is either delivered or explicitly recorded as an accepted gap.
- Bounded entry count, text length, and expiry are enforced as typed refusals.
- `architecture/tui.md`, `architecture/core.md`, `architecture/session.md`, the
  corresponding skills, and `docs/tui.md` describe the delivered contract, and the
  two steering mechanisms are distinguished.
- ADR-0035, ADR-0036, ADR-0037, and ADR-0038 are accepted, not merely
  proposed. **All four are accepted (2026-10-18). ADR-0036's acceptance
  discharges the M006 and M007 copy gates; both plans are now written, and **M006
  is `closed`**. **M007 is also `closed`** — its §2 decision was taken as Option A
  and recorded in ADR-0039, which is **proposed, not accepted**. Acceptance is a
  maintainer action and is the one thing still outstanding across this workstream;
  it does not block M007's closure, because the implementation was authorized by
  the decision itself.**
- An entry interrupted mid-delivery resumes **at its own position**, so a transient
  error or a concurrent reorder cannot demote a prompt to the back of the user's own
  queue (C001). Met for `release_delivery` and for startup reconciliation. The
  `failed`-revival path is the documented exception (§10) and is an open maintainer
  decision, so this bullet is not met for `retry_failed`.

Roadmap closure does **not** require the deferred items in §3 to be built.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 1 | closed | `plans/implementation/user-prompt-queue/001-turn-contention-refusal-correctness.md` | `plans/closure/user-prompt-queue/001-status.md` | none |
| 2a | closed | `plans/implementation/user-prompt-queue/002a-durable-queue-storage-and-read-projection.md` | `plans/closure/user-prompt-queue/002a-status.md` | none |
| 2b | closed | `plans/implementation/user-prompt-queue/002b-queue-mutation-operations-and-authority.md` | `plans/closure/user-prompt-queue/002b-status.md` | M002a closure (discharged); ADR-0037 acceptance (discharged 2026-10-18) |
| 3 | **closed** | `plans/implementation/user-prompt-queue/003-queue-delivery-through-turn-admission.md` | `plans/closure/user-prompt-queue/003-status.md` | M002b closure **discharged**; ADR-0038 acceptance **discharged 2026-10-18** (Option B) |
| 4 | **closed** | `plans/implementation/user-prompt-queue/004-queue-box-list-select-delete-and-indicator.md` | `plans/closure/user-prompt-queue/004-status.md` | M003 closure **discharged**; interface dep **corrected and discharged** — the named `panel-convergence M004` does not exist and M004 duplicates the established chrome |
| 5 | **closed** | `plans/implementation/user-prompt-queue/005-queue-box-add-edit-and-reorder.md` | `plans/closure/user-prompt-queue/005-status.md` | M004 closure **discharged**; §3(b) **measured and recorded** — and **superseded by C001**, which fixes the measured behaviour rather than documenting it |
| 6 | **closed** | `plans/implementation/user-prompt-queue/006-send-now-steering-from-the-queue.md` | `plans/closure/user-prompt-queue/006-status.md` | None — none outstanding. M004 **and** M005 closure **discharged**; ADR-0036 acceptance **discharged** (it names this plan path). **No migration**; `STORAGE_LAYOUT_VERSION` unchanged at 81. `s` ships with a second `delivered` path (`mark_steer_delivered`) and a three-outcome result enum |
| 7 | **closed** | `plans/implementation/user-prompt-queue/007-delayed-queue-delivery.md` | `plans/closure/user-prompt-queue/007-status.md` | M004 **and** M005 closure **discharged**; ADR-0036 copy boundary **discharged**; the §2 wakeup-owner decision **taken as Option A** and recorded in ADR-0039 (**proposed**, awaiting maintainer acceptance). **Takes migration v82** — `STORAGE_LAYOUT_VERSION` is now 82 |
| 8 | proposed | — | — | M004 + M005 closures (interface) — both discharged, so M008 is eligible and remains explicitly deferrable |
| C001 | **closed** | `plans/implementation/user-prompt-queue/C001-released-delivery-order-displacement-corrective.md` | `plans/closure/user-prompt-queue/C001-status.md` | None — none outstanding. Class **invariant**; no migration, no protocol change. Two defects were **found and reported, not fixed**, per C001 §14: `retry_failed`'s revive, and the pre-existing second-displacement collision (both §10) |

M002 was split into M002a and M002b on 2026-10-18 after accepting ADR-0035. The
numbers are not reused: M002a and M002b keep `002` as their plan prefix because
they are one roadmap milestone divided at its authority boundary, and closure
records will be `002a-status.md` and `002b-status.md`.

## Milestone status change record

- 2026-10-17 — Roadmap created as `proposed` at baseline `18c3dca9`, with M001
  written and registered as `ready`. ADR-0035 and ADR-0036 created as `proposed`.
  M002 through M008 are deliberately unplanned: each depends on a closure or an ADR
  acceptance that does not exist yet, and writing them now would encode guesses
  about contracts that are still open. M001 is written because it is dependency-
  clean, fixes a live text-loss defect independently of the queue, and is required
  to be closed before the queue is built on top of it.
- 2026-10-17 — M001 **closed** at baseline `18c3dca9`
  (`plans/closure/user-prompt-queue/001-status.md`). Contention now preserves
  active-turn state, every recoverable refusal restores the composed text through
  one shared bounded shape, and the submit path uses the shared request-state
  guard. M002's hard dependency on M001 is discharged; M002 remains unplanned and
  blocked only on ADR-0035 acceptance, which is a maintainer action and confers no
  implementation authority until taken. Two findings are carried forward rather
  than absorbed: the `agents_to_dtos` pre-flight failure still does not release the
  turn, and the plan's §11 verification list contained two filter strings that
  select zero tests.
- 2026-10-18 — Roadmap moves `proposed` → `active`. **ADR-0035 accepted** by
  owner direction, discharging the blocker M001's closure had narrowed it to. The
  acceptance audit did not simply clear M002: it found that ADR-0035's authority
  clause — "the same controller predicate that governs turn admission" — is
  **unsatisfiable while a session is idle**, because the durable lease is
  turn-scoped and deleted at turn end, leaving only the session-unbound `is_local_owner_broad` identity
  bypass. Since mutating a queue *between turns* is
  the primary use, the authority model is a canonical ownership-boundary decision
  and needs its own ADR. Consequently:
  - **M002 is split** into **M002a** (durable storage + read projection,
    infrastructure, `ready`) and **M002b** (mutation operations + mutation
    authority, infrastructure, `blocked`). M002a is dependency-clean under every
    ADR-0037 outcome, so real work proceeds now.
  - **ADR-0037** is raised as `proposed`. It is not accepted, so M002b does not
    start; the ADR-0030/ADR-0034 precedent applies.
  - M003's dependency is restated as M002b, and M003 inherits one bounded
    decision: the v80 `status` `CHECK` enumerates all five roadmap values even
    though three are unreachable until M003, because SQLite cannot `ALTER` a
    `CHECK` and narrowing it would force a rebuild of a durable user table.
  - `run_at_ms` was deliberately **omitted** from the v80 table. Nothing in M002a
    writes it, a durable column with no producer is the claim-without-a-hook
    defect this repository treats as a bug, and M007 adds it with an in-place
    `ALTER TABLE … ADD COLUMN`. This is a recorded narrowing of
    ADR-0035's descriptive table sketch, not a silent one. (The migration version
    was v81 at the time of this entry; accepting ADR-0037 the same day spent v81
    on the queue-owner binding, so `run_at_ms` is **v82**. Superseded below.)
  - Two ADR-referenced seams were corrected against source: the read is authorized
    with `Capability::SessionRead` rather than the broader `AgentInvoke` that
    `human_shell_promotions_get` uses, and `is_controller_bypass` gains no queue
    authority.
- 2026-10-18 — M002a **closed** at baseline `18c3dca9` plus the M001 tree
  (`plans/closure/user-prompt-queue/002a-status.md`). **v80** landed: the durable,
  bounded, expiring `user_prompt_queue_entry` table with a schema-enforced
  `UNIQUE (session_id, ordinal)`, typed refusals for every bound, the lazy
  transactional expiry sweep, and the `UserPromptQueueList` read under
  `Capability::SessionRead`. **It delivered no user-visible capability**: no
  mutation exists, so the read has no producer and the queue always reads empty.
- 2026-10-18 — **ADR-0037 accepted** by owner direction, selecting **Option B**
  (durable, session-scoped queue-owner binding) and adopting the M002a/M002b split
  in the same decision. Consequences:
  - **M002b is unblocked** and moves `blocked` → `ready`. Its blocker was created
    *by* accepting ADR-0035 and is now discharged by accepting ADR-0037.
  - The queue's mutation authority is a durable per-session owner row, **not** the
    turn-controller lease and **not** `is_controller_bypass`. Mid-turn the owner
    and the controller must both match the caller; a mismatch is a typed refusal,
    never a precedence rule. This is the sharp edge ADR-0037 exists to resolve.
  - **A pre-acceptance correction was recorded in ADR-0037 itself.** Its draft
    claimed the owner binding would be folded into **v80**; M002a had already
    shipped v80 with the queue table alone, and a shipped migration is never
    edited. The claim was corrected to **v81** while the ADR was still `proposed`
    and the correction is stated in the accepted record's header so the difference
    between draft and accepted text is visible. The **decision** is unchanged.
  - `run_at_ms` therefore moves from v81 to **v82** (M007), since v81 is now the
    owner binding.
  - The predicate's call shape is a **named** `ControllerScope` enum rather than
    the `Option<&str>` binding a blocked draft suggested: `None` cannot
    distinguish "an unowned queue" from "a turn operation with no turn", and those
    require different refusals.
  - The adoption policy ADR-0037 deferred is decided: an **`Absent`** queue adopts
    atomically on first add (otherwise nothing could ever create the first entry),
    but an **`Unowned`** queue is refused for every mutation and repaired only by
    the explicit adoption operation — so an invalidated queue can never silently
    become owned by whoever touches it next.
  - **ADR-0036 remains `proposed`** and still gates M006 and M007's copy boundary.
    This acceptance does not touch it.
- 2026-10-18 — **M002b closed**
  (`plans/closure/user-prompt-queue/002b-status.md`). ADR-0037 was confirmed
  `accepted` (Option B) before any code was written, so the ADR-0030/ADR-0034
  "do not start on a proposed ADR" rule no longer blocked this milestone. **v81**
  landed carrying only `user_prompt_queue_owner` — additive, and v80 was left
  untouched. The four mutations reached real producers, and the queue stopped
  being permanently empty, but **no user-visible capability was delivered**: there
  is still no command, dialog, key, row-0 change, or projection field, so nothing
  a person can press reaches the six new operations. `is_controller_bypass` gained
  no queue authority — its early return is guarded by
  `matches!(scope, ControllerScope::Turn { .. })` and a test proves it by
  execution.
  - The adoption policy ADR-0037 deferred to M002b is settled and audited: **the
    calling principal becomes the owner** on first add — not the session creator
    and not the local interactive owner.
  - `ControllerScope` shipped with **three** variants, not the plan's two.
    Routing `adopt` through `QueueMutation` would refuse it with `QueueUnowned`
    and make ADR-0037's mandated repair path unreachable, so `QueueAdoption` asks
    the question adoption actually asks. A failing test surfaced this; the code
    was wrong, not the test.
  - Two real bugs were caught by the new tests and fixed before closure rather
    than shipped: `delete` never renumbered its survivors (permanent ordinal gap),
    and `is_permutation` had an inverted `HashSet` duplicate check.
  - M003's hard dependency — M002b closure — is **discharged**. M003 inherits one
    open **decision**, not a blocker: v80's `status` `CHECK` enumerates five values
    while only `pending`/`expired` have producers, and SQLite cannot `ALTER` a
    `CHECK`, so M003 must either make all five reachable or rebuild the table.
  - **ADR-0036 remains `proposed`** and still gates M006 and M007's copy boundary.
    Neither this milestone nor ADR-0037's acceptance touched it.
- 2026-10-18 — M003 **planned and registered as `blocked`**, and **ADR-0038
  raised** (`plans/adrs/ADR-0038-queue-delivery-execution-authority.md`,
  `proposed`). Planning M003 discharged its stated hard dependency (M002b's
  closure) and immediately exposed the decision ADR-0035 and ADR-0037 both left
  open: **as whom is a queued entry submitted?** The daemon may originate an
  admission, but the repository's only such path (`submit_idle_continuation_turn`,
  ADR-0031) reconstructs authority from durable attribution via
  `AuthenticatedPrincipal::reconstructed`, which needs five fields, while the v81
  owner row stores one (`owner_principal`). Delivering under a synthetic daemon
  identity would take the controller lease from the real owner and lock the owner
  out of its own queue at its first mid-turn mutation — a failure that is
  invisible until someone uses the feature that now works. ADR-0038 recommends
  Option B (resolve attribution from team state, no migration) over Option C
  (a self-contained owner row, which **contests migration v82 with M007's
  `run_at_ms`**). Consequence for the registry: **M003's blocker is no longer a
  closure — it is an ADR acceptance**, and the ADR-0030/ADR-0034 rule applies.
  - The **inherited `status` `CHECK` decision is answered in the plan**: make all
    five values reachable rather than rebuild a durable user table.
  - Research also found **three projection behaviours that break the instant
    `delivering` has a producer**: `list_live` sweeps *and* selects
    `status = 'pending'`, `check_capacity` counts only `pending`, and the expiry
    sweep covers only `pending`. A `delivering` row that is never resolved is
    therefore invisible, uncounted against the backlog bound, never expired, and
    never retried. This is now M003 work package B, framed as correctness rather
    than cleanup.
  - Two more facts recorded rather than assumed: the daemon has **no periodic
    session-observation loop** (its three `tokio::time::interval` loops are
    projection retention, opt-in provider refresh, and opt-in TTS), so M003 must
    introduce its own trigger and may not introduce a general poller; and the
    queue has **no `CoreEvent`**, reloading only on the session-change edge, so
    delivery without a push path would leave a frontend showing a delivered entry
    as waiting.
  - The reaper (`spawn_turn_reaper`) is the confirmed post-terminal hook — it
    already runs after `store.release` and is keyed on `(session_id, turn_id)` —
    but it clones only `sessions` and `pool`, so calling back into the daemon
    needs the `Weak<CoreDaemon>` upgrade pattern already proven at
    `src/core/daemon.rs:575-580`.
  - **M004 remains the next milestone needing no ADR.** Its M003-closure
    dependency is **discharged**; its only remaining blocker is the
    panel-convergence chrome helper.
- 2026-10-18 — **M003 closed** (`plans/closure/user-prompt-queue/003-status.md`).
  The workstream's **first observable behaviour**: a queued prompt is claimed in
  ordinal order, admitted through the ordinary `TurnSubmit` path **as the queue
  owner**, and reflected back in the projection. It took **no migration**, so
  `run_at_ms` stays v82 and the v82 contest was resolved by not arising rather
  than by deciding it.
  - Two durable facts the milestone had to fix rather than add: the three
    `status = 'pending'` literals (read projection, backlog bound, expiry sweep)
    are now one constant, because `delivering` had no producer before this
    milestone and a stranded row would otherwise be invisible, uncounted, and
    unexpired at once; and `CoreEvent::UserPromptQueue` is classified
    `ClientLocal`, never `Safe`, because it carries prompt text and
    `event_log.publish` also feeds the projection sink.
  - A real defect was found and fixed: `claim_next_for_delivery` re-read "the
    lowest `delivering` row" after claiming, so with a stranded row at a lower
    ordinal it returned the *stranded* entry — one prompt executed and another
    recorded against it. The regression test was proven non-vacuous by
    re-injecting the original query and observing the failure.
- 2026-10-18 — **ADR-0038 accepted (Option B)** and **M003 moved `blocked` →
  `active`**. This is the third consecutive instance of the same shape: planning a
  milestone discharges its stated dependency and immediately exposes the next
  canonical decision, which must be settled by an ADR before the plan may start
  (M002b → ADR-0035's authority clause → ADR-0037; M003 → *as whom* → ADR-0038).
  Option B was selected over Option C because it needs **no migration**, which
  removes the v82 contest entirely rather than deciding it: `run_at_ms` stays
  **v82** for M007, and M003 takes no migration number.
  - The accepted trade is explicit: `AuthMethod` and `TransportClass` become a
    **delivery classification**, not a record of what originally authenticated the
    owner, so audit loses that forensic fidelity. A queue delivery has no
    originating connection, so a per-connection classification would be a
    fabrication.
  - Six clauses are settled regardless of attribution source: submission **only**
    through `TurnSubmit`; a **deterministic** submission id derived from the entry
    id, so at-most-once is inherited from the existing receipt rather than
    reimplemented; a **named** entry with exactly two guarded callsites; authority
    **re-authorized against current team state** at delivery time; delivery runs
    **as the queue owner**, never synthetic and never `is_controller_bypass`; and
    ADR-0036 stays untouched.
  - The v82 row in §10 is now **Resolved** rather than merely recorded, and §11's
    completion clause no longer counts ADR-0038 among the proposed ADRs.
- 2026-10-18 — **ADR-0036 accepted (Option A)**, and **M004 and M005 written and
  registered**. This is the fourth instance of the same shape: planning a
  milestone discharges its stated dependency and immediately exposes either the
  next canonical decision or a **false premise in its own dependency list**.
  Here the decision came first, and the premise failed.
  - **ADR-0036 accepted, Option A** — reuse the existing `TurnSteer` backend and
    state the guarantee as **acceptance into the turn's steering channel, not
    delivery**. The accepted trade is explicit: the feature is best-effort and
    this decision does not change that; a failed steer is **retained in its
    original position**, which is strictly safer than dropping it and keeps the
    queue the single durable home for user-authored text. "Force" is forbidden in
    copy. **M006 and M007's copy gates are discharged**; both remain blocked
    only on M004 closure.
  - **ADR-0037's exclusion is confirmed, not assumed.** It states *"Whether
    ADR-0036's steering path should require the same owner binding is out of
    scope here; `TurnSteer` keeps its existing controller predicate"*
    (`plans/adrs/ADR-0037-idle-session-queue-mutation-authority.md:247-248`).
    Steering governs injection into a **running** turn, which by definition has a
    live controller lease; ADR-0037 governs mutation **between** turns. They are
    deliberately not unified.
  - **The stale cross-reference labels in ADR-0035 and ADR-0037 were refreshed**
    from `(proposed)` to `(accepted 2026-10-18)`. Those are status labels on a
    *referenced* document, not decision content; leaving them would have kept
    M006 falsely gated. Each edit says so in place.
  - **M004's named interface dependency does not exist.** The roadmap pointed at
    `panel-convergence M004` for the shared chrome/geometry helper. That
    addendum defines only M001/M002/M003 — `M004` has **zero occurrences** — and
    its M003 explicitly defers the helper out as *"its own milestone, written
    against a settled shape"*, which was never opened. The **geometry** half has
    in fact already landed and is de-duplicated (`centered_rect`,
    `src/tui/components/component.rs:290`); only chrome is missing, and thirteen
    widgets already follow a per-widget `Chrome::{Owned, Hosted}` shape. **The
    registry's claim that M004's only remaining blocker was this helper
    overstated it**, and M004 is therefore `ready` under the roadmap's own
    fallback: duplicate the established chrome, do not fork a helper, do not
    open that milestone from here.
  - **Two roadmap descriptions of the TUI were corrected against source**, not
    softened. (1) §4 said row 0's right-hand side is reserved for a named escape
    gesture; `stop_hint` (`src/tui/app/render.rs:1690-1703`) is a
    **left-aligned `" · "`-joined suffix** on the same line. (2) M004's deliverable
    boundary implied a `Dialog` variant with key arms; the legacy per-variant key
    router is `#[cfg(any())]`-disabled
    (`src/tui/app/mod.rs:4364-4368`) and `FocusManager` is the sole key
    authority, so an arm there would compile and do nothing. Only the two
    `From<Dialog>`/`From<DialogType>` impls are exhaustive for a new variant.
  - **M005's planning surfaced two store-level facts that are now named risks**,
    both from M003's `user_prompt_queue.rs` and neither previously recorded:
    reorder's permutation is validated against the **pending** set only while the
    client projection carries pending **and** delivering, so a client that sends
    every id it rendered is refused with `OrderMismatch` the moment any entry is
    delivering; and `renumber_dense` displaces every row session-wide and
    status-blind before densifying, so a `delivering` row's ordinal jumps above
    every pending row — no `UNIQUE` collision, but a **released** delivery
    returns to `pending` at the back of the queue. The second is closed M002b/M003
    behaviour; M005 must test and record it, and a store fix is a maintainer
    decision rather than a TUI workaround.
  - **Observer blocking needs no work and must stay that way.** There is no role
    field on `Command`; `is_observer_allowed_command` is a narrow read-only
    allowlist evaluated at dispatch and failing closed
    (`src/tui/app/state/observe.rs:476-528`). `/queue` and every subcommand are
    already blocked by omission, and
    `tests/identity_m003_daemon_authorization.rs:1472-1484` already requires
    `queue`, `queue-add`, `queue-edit`, `queue-delete`, and `queue-reorder` never
    to be observer-allowed. Both plans extend that assertion rather than adding
    an entry.
  - M008 remains `proposed` and explicitly deferrable. No milestone is written
    speculatively, and **M004 and M005 take no migration number** — `run_at_ms`
    stays **v82** for M007.

- 2026-10-18 — **M004 implemented and closed** (`plans/closure/user-prompt-queue/004-status.md`).
  `/queue` is registered and dispatches, `Dialog::Queue`/`DialogType::Queue` exist with both
  exhaustive `From` arms and the round-trip row, `components/dialogs/queue.rs` is the
  FocusManager-owned list, `commands/queue.rs` owns open/refresh/delete, and row 0 carries a
  left-aligned `" · "`-joined `queued` suffix. M005's only blocker is discharged, so M005 moves
  `blocked` → `ready`. Four things happened during implementation that the planning did not
  say, all recorded in the closure record rather than smoothed over here:
  - **`Dialog::Queue` has no key arms, by design.** The legacy per-variant router is
    `#[cfg(any())]`-disabled and `FocusManager` is the sole key authority, so an arm there
    would compile and do nothing. `open_dialog` got an explicit no-op arm so the no-op is
    declared rather than reached by falling through, and `dock_target` still has no queue arm.
  - **Selection follows `entry_id`, not the index.** A projection that lost a row would slide
    the cursor onto a different prompt, and the `d` pressed next would remove something the
    user never looked at. Clamping handles the case where the selected entry is the one that
    disappeared.
  - **The "no second read on delete" guard was vacuous twice before it measured anything.** It
    first counted transport calls immediately after `on_key` returned, which races the
    `spawn_tui_task` worker. After that was fixed it still passed against an injected second
    read, because everything the test drove ended at the *request* — `start_queue_delete`
    answers on `tui_cmd_tx` and no frontend test dispatches it — so the apply path was never
    executed. An earlier source-grep version of the same guard was deleted rather than
    repaired: it tripped on the module's own doc comment, so it measured documentation, not
    behaviour. The guard now measures requests at a recording fake transport and drives the
    apply directly; the injected read fails it with `["list", "delete:b", "list"]`.
  - **A wrong wire code was caught by the build, not by review.** The first
    `refusal_message` draft mapped `user_prompt_queue_entry_expired`, which does not exist —
    the `entry_` prefix was carried over from `user_prompt_queue_entry_delivering`, and the
    real code is `user_prompt_queue_expired`. All six mapped codes are now pinned against
    `UserPromptQueueError::code()`, and each is asserted to have its own sentence so a pinned
    constant cannot be dead. This is the reason the TUI matches codes as literals rather than
    importing the daemon's enum: with the enum imported, the rename and the mapping would move
    together and the mismatch would become silent.
  A second render path (`Widget for &QueueDialog`) was written and then **removed**: nothing in
  production uses it, two render paths for one box is the divergence this codebase documents
  elsewhere, and its presence made `dialog.render(...)` resolve to the wrong trait's method.
- 2026-10-18 — **M005 implemented and closed**
  (`plans/closure/user-prompt-queue/005-status.md`). The `/queue` box can now author
  (`a`), correct in place (`e`), and reorder (`[`/`]`) alongside M004's list and delete.
  No migration, no protocol change, no daemon change, and no store change: the three
  requests and the response shape already existed from M002b/M003, so the entire
  milestone is `src/tui` plus tests and docs.
  - **The text step is guarded structurally, not per letter.** `handle_key` matches
    `is_text_step()` **first, as a separate match** and returns from it, which makes the
    browsing arm unreachable while a composer is open. The plan asked for
    `!is_text_step()` on each action letter; that shape is what eventually ships a sixth
    action with a forgotten guard, and the failure is `d` deleting a prompt the user was
    typing. No shared `App`-level `captures_text` predicate was opened — the stop
    condition holds and the `ConnectDialog::is_text_input_step` precedent is followed.
  - **Composed text lives in the widget, not `DialogState`.** There is therefore no window
    in which a later `/queue` can commit text an abandoned step left behind.
  - **§3(b) is settled by observation, and the answer is sharper than the plan predicted.**
    A released `delivering` entry does not merely resume *after* the others — it keeps the
    displacement ordinal (`USER_PROMPT_QUEUE_RENUMBER_BAND_OFFSET` = 1 000 000) and sits
    outside the dense band entirely, because a release restores the status and not the
    ordinal. Recorded as accepted-and-documented; changing `renumber_dense` reopens closed
    M002b/M003 and is a maintainer decision.
  - **The plan's shifted-key reasoning was a wrong premise, and the real answer is stronger.**
    It reasoned that `docks_as_viewer()` defaults to `false` so `Shift+K`/`Shift+J` stay
    available. This box **overrides** it to `true`. The keys survive anyway because
    `App::dock_target` has no `DialogType::Queue` arm, so the docking-steal branch is dead
    regardless of the viewer flag — and that is now asserted rather than assumed, because an
    omission is exactly the kind of property that silently stops holding. `[`/`]` were chosen
    over `Shift+K`/`Shift+J` regardless: a shifted variant of the movement key that does
    something *other* than move is a pairing people misfire.
  - **The permutation is built from `pending` rows only**, which is §3(a) and the most likely
    defect in the milestone. Proven non-vacuous at both levels: with the filter removed, the
    unit test yields `["a","c","b"]` instead of `["c","a"]` and the integration test sends
    `reorder:a,c,b` — naming the `delivering` row and earning `order_mismatch` from the
    daemon.
  - **One local refusal is a deliberate exception to M004's "let the daemon answer".** A
    `delivering` selection cannot be reordered daemon-side at all: the permutation omits the
    row, the write **succeeds**, and nothing moves — a silent no-op rather than an honest
    refusal. There is no daemon call that can express "this row is not yours to move".
  - **A live bug was found by the close-during-mutation test.** The shared refusal helper
    returned `None` for *both* success and stale/cancelled, so a completion arriving after
    `close_dialog` cancelled it was treated as a success and wrote its projection anyway —
    precisely what the request generation exists to prevent. Fixed with an explicit
    three-state `RefusalOutcome`; proven by deleting the three `cancel()` calls, which fails
    the test with the cancelled projection written.
  - **`/queue-adopt` is not named anywhere, on purpose.** The first draft of the `unowned`
    sentence told the user to "run `/queue-adopt`". The protocol request exists; **no TUI
    command registers one**. Naming it would have been this milestone's own version of the
    dead-key dishonesty it exists to avoid. The sentence now says the repair is a separate
    explicit operation outside the box.
- 2026-10-18 — **M006 and M007 planned; C001 corrective opened.** Planning only; no
  implementation. All of M004/M005's and ADR-0035/0036/0037/0038's gates are discharged,
  so the next milestones were written rather than left speculative — which is exactly the
  condition this roadmap set for opening them ("no milestone is written speculatively").
  - **M006 `ready`** (`006-send-now-steering-from-the-queue.md`). It is the only milestone
    in this roadmap whose authorization is a precondition rather than a coincidence of
    timing: ADR-0036 names `plans/implementation/user-prompt-queue/006-*` in its own text.
    The backend exists, is authorized, and is unreachable; the five outcomes already exist
    as distinct wire codes; and `app.session_state.active_turn_id` already carries the turn
    identity, so M006 adds no projection field. **No migration**: on `Ack` the entry is
    marked `delivered` and bound to the accepting turn, using the `delivered_turn_id` and
    `delivered_at_ms` columns v80 already created.
  - **The deferred UX decision is now made.** ADR-0036 explicitly reserved it — "whether a
    successfully steered entry remains visible in the queue … is a UX decision for the
    milestone that implements the box" — and forbade M004/M005 from deciding it. Leaving
    the entry `pending` would run the same instruction **twice**, once steered and once
    claimed by M003 as a fresh turn, which is the duplication ADR-0036's own drivers
    forbid. The rejected alternative (a new `steered` status) cannot be added cheaply:
    v80's `status` `CHECK` is not `ALTER`-able, so SQLite would force a rebuild of a
    durable user table.
  - **M007 `blocked` on one decision** (`007-delayed-queue-delivery.md`). Its stated basis
    is false. §5 claimed advancement rides "an existing periodic observation point"; no
    such point exists. Measured: the daemon's only `tokio::time::interval` loops are
    projection-replay retention (300 s, spawned only when the projection seam is enabled),
    opt-in provider refresh, and opt-in TTS, and `submit_queued_prompt_turn` has exactly
    two permitted callers — `deliver_next_queued_prompt` and
    `reconcile_stranded_queue_deliveries`. Neither fires for an idle session holding a
    scheduled entry, so due-ness enforced only in the store query delivers an entry **late
    or never**. Three options are costed in the plan with a recommendation (a bounded
    queue-scoped tick that parks when nothing is scheduled); the choice is a maintainer
    action, and adding a caller to the delivery entry is a **recorded** decision under
    `scripts/check_queue_delivery_callsites.py`, not a quiet third call site.
  - **Two M007 premises corrected against source.** `parse_duration` already exists
    (`src/background_task_migration.rs:11-34`) and accepts `30s`/`5min`/`10m`/`2h`/`1d`, so
    it is reused rather than duplicated — and it does **not** parse `1 hr`, which this
    roadmap used as its own example, so copy must say `1h`; a bare number must be refused,
    never guessed as seconds. And a due time at or after an entry's own expiry could never
    run, so that combination is a typed refusal rather than a silent clamp.
  - **C001 opened as an invariant corrective**
    (`C001-released-delivery-order-displacement-corrective.md`), answering the maintainer
    decision M005's closure recorded. M005 measured a **render** symptom; planning C001
    measured the **delivery** symptom — `claim_next_for_delivery` selects
    `ORDER BY ordinal ASC`, so the returned entry is delivered last, not merely shown last
    — and found **three** affected paths, not one: `release_delivery`, `retry_failed`, and
    `resolve_stranded_delivery(Retry)` all restore `pending` with no ordinal write. That
    contradicts ADR-0035's own "a trigger that fires out of order must not reorder the
    user's work", so it is a defect against **accepted** architecture and needs no new ADR.
    The fix takes **no migration**: `renumber_dense` writes dense ordinals for the whole
    *live* set and preserves each non-pending row's existing rank, which leaves the uniform
    displacement step — and therefore `UNIQUE (session_id, ordinal)` — exactly as it is.
    The client's permutation contract is unchanged; still pending ids only.
  - **Two M005 tests asserted the defect** and are to be **replaced** by
    position-preservation tests. They were measurements of an accepted-and-documented
    outcome rather than contract requirements, which is what makes correcting them safe;
    leaving them would describe a bug as correct in a green suite. M005's closure record is
    **not** rewritten, and the `docs/tui.md` sentence telling users to add a prompt last
    rather than reorder an older one is removed, because C001 makes that advice wrong.
  - Migration numbering is unchanged: **v82 stays M007's** `run_at_ms`, `STORAGE_LAYOUT_VERSION`
    is 81, and C001 takes none.
  - **M008 is now eligible** — M004 and M005 are both closed — and remains **explicitly
    deferrable**. Deferral is now a choice rather than a blocked state, and is stated as
    such rather than left implied.

- 2026-10-18 — **C001 closed; two further defects found and reported, not fixed.**
  Implementation of the invariant corrective. `merge_operable_into_live` folds the
  client's desired pending order with each non-pending live row held at **the rank it
  already had** — decided as *preserve existing rank*, never pinned to an end, because
  pinning a released entry to the tail is the defect itself — and `renumber_dense` writes
  that merged live set. The displacement step is **unchanged**, terminal rows included:
  they are not live but still hold ordinals under a session-wide `UNIQUE`, so excluding
  them would reintroduce the collision the band offset exists to prevent.
  - Because a `delivering` row now *receives* a dense ordinal, `release_delivery` and
    `resolve_stranded_delivery(Retry)` needed **no change at all** and are now
    **order-neutral** — a released queue is identical to the queue before the release.
  - **The client permutation contract is unchanged**: still pending ids only, verified by
    refusing a permutation that names a `delivering` row.
  - **No migration.** `STORAGE_LAYOUT_VERSION` stays **81**; v82 stays M007's.
  - The two M005 tests that asserted the defect are **replaced** by position-preservation
    tests, and the `docs/tui.md` workaround callout is **removed in full** — its premise
    is now false, and leaving the framing would teach users to work around a fixed bug.
    M005's closure record is **not** rewritten; §7 M005 is annotated, not edited.
  - **Two defects found and reported rather than fixed, per C001 §14**, and both now carry
    their own §10 rows:
    1. **`retry_failed` is not reached by the merge.** C001's §6 claims all three return
       paths are covered with no change to those functions; that claim is **falsified** for
       this one. It revives a row that was **terminal** during the reorder, so the row is
       absent from the live set the merge reads. No production caller, and a `failed` entry
       is absent from the client projection — so nothing is lost and nothing was observed
       to move. Where such a row belongs is an **open maintainer decision**, not a detail
       to pick in passing.
    2. **The uniform displacement collides with itself on a second renumber.** Pre-existing
       and independent of the merge: the displacement `UPDATE` is byte-identical before and
       after C001, and the collision happens inside that statement. A uniform offset is only
       collision-free while no row already sits at or above the band. Minimal reproduction,
       reachable from the queue box alone: queue three prompts, reorder, let one fail,
       reorder, **reorder again** — the third is refused with a storage error. C001 §14
       stops the milestone on exactly this, and a correct fix (a monotonic band generation,
       or compacting terminal rows) is a design decision.
  - **Verification shape.** The property test is an **exhaustive** enumeration of all
    **1 554** sequences of length 1–4 over **six** operations, not a random sample — the
    state space is small enough to cover completely, and it is deterministic. A sixth
    operation, `ClaimHold`, was **added after** injection (i) showed the enumeration passed
    against the *unfixed* store: with no step that leaves a row `delivering`, no two
    operations ever straddled a reorder with one, which is the entire defect scenario. That
    hole is closed and the injection now fails the property test too.
  - One property assertion was **narrowed after it failed correctly**: "the live set is
    always dense `0..n`" is false for reasons that predate and are unrelated to C001 — a
    row going terminal leaves a hole only the next reorder or delete closes, and `add`
    appends over `MAX(ordinal)` across **all** rows. The assertion now states the property
    C001 actually guarantees: **no live row is ever stranded above the band**.

- 2026-10-18 — **M006 closed; send-now ships as a third `delivered` path, and the
  plan's transport guidance was corrected by measurement.**
  - **`s` in `/queue` hands a queued prompt to the turn that is already running**, with
    no migration: `STORAGE_LAYOUT_VERSION` stays **81** and **v82 stays M007's**.
  - **The deferred acceptance decision shipped as planned.** `mark_steer_delivered`
    moves `pending` → `delivered` and stamps `delivered_turn_id` with the accepting turn,
    using columns v80 already created. It is guarded on `status = 'pending'` for the same
    at-most-once reason as `mark_delivered`: a row M003 already claimed is running as its
    **own** turn, so settling it as steered too would record one prompt delivered twice.
  - **The request carries `entry_id` and nothing else.** No prompt text, and no
    `turn_id` — the daemon loads the stored text and resolves the active turn from its own
    runtime handle, so a client can neither submit unattributed text nor steer a turn that
    has already ended. This is what makes the plan's "retained-on-failure" contract fall
    out for free: on any refusal nothing is written, so the prompt is still queued.
  - **One correction to the plan's transport guidance.** The plan said to route by
    `App::is_ws_attach_transport()`. Measurement says otherwise: send-now issues
    `CoreClient::request`, the same call M004/M005 already make and the one that works on
    both attach transports. `is_ws_attach_transport()` governs `send_remote_message`
    answer channels; adding a gate here would have been cargo-cult dressed as caution.
  - **An outcome the plan's two-valued shape did not have.** The steer happens *before*
    the settlement, so "the turn refused it" and "the turn took it but the queue could not
    record that" are different states. `AcceptedNotSettled` reports the second; reporting
    it as an error would tell the user to resend a prompt that has already run — the
    duplication ADR-0036 forbids. It is audited as `accepted_unsettled`, the only record
    of a delivery with no matching `turn_admission_receipt`.
  - **Send-now is deliberately not a `QueueMutation`.** ADR-0037 excludes steering from the
    durable owner binding, so the *handover* is gated by the turn controller alone. Queue
    ownership is re-asked **inside** `mark_steer_delivered`'s transaction, where it answers
    a different question — "may this principal change this queue?" — about a different
    write. `steer_channel_code` is now shared with `TurnSteer`, so the two steering paths
    cannot drift apart on what a full or closed channel means.
  - **Both §10 guards proven non-vacuous.** Inverting the text-step guard failed **2**
    tests (M006's own, and an M005 add test — `s` acted instead of typing, yielding
    `"adf"` for `"asdf"`). Deleting the `close_dialog` cancel failed **2** (the
    stale-completion test, with the projection written after close, and the in-flight
    cancel test). Both reverted, zero stray markers.
  - **A hint guard from M005 rejected this milestone's own box** before a test was
    adjusted — the exact failure mode that guard exists for, and now the second time it
    has earned its place.
  - `docs/tui.md`'s "It cannot send a queued prompt early" bullet is **gone**, and its
    replacement states the handover plainly rather than as an override. M005's and M004's
    closure records are **not** rewritten; §7 and the `architecture/` docs are annotated.

### 2026-10-18 — M007 closed (delayed queue delivery)

**Milestone:** 7. **Class:** capability. **Migration:** v82 —
`STORAGE_LAYOUT_VERSION` 81 -> 82. v80 and v81 untouched.

**The decision this milestone was blocked on was taken: Option A**, a bounded
queue-scoped advance tick, recorded in
`plans/adrs/ADR-0039-queue-advancement-wakeup-authority.md` (**proposed**;
acceptance is a maintainer action). The tick computes the earliest due instant
across sessions, sleeps exactly until it, and parks on a `Notify` with **no timer
armed** when nothing is scheduled.

**What changed in the roadmap's own text**, because the milestone falsified it:

- §5's "advancement rides an existing periodic observation point" was already
  marked falsified during M007 planning; it now also records that the
  corresponding **exit condition could not stand as written** and states what the
  implementation actually guarantees.
- §7 Milestone 7 gained two recorded deviations (a second authoring step rather
  than an inline suffix; delivery as a floor rather than a promise) and one defect
  found and fixed (the tick's zero-delay spin).
- §10 gained rows for the resolved reorder/timer precedence, the spin defect, and
  v82's landing.
- §11's delayed-delivery bullet and §12's M007 row are marked met/closed.

**Deliberately not done.** The M003 lock-upgrade deadlock is unchanged: the
maintainer decided to land it and file a corrective rather than widen M007 into a
store-wide transaction-shape change. Its §10 row stands, annotated, as written by
M006.
