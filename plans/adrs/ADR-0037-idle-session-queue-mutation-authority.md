# ADR-0037: Idle-Session Prompt Queue Mutation Authority

Status: accepted

Date: 2026-10-18 (authored); accepted 2026-10-18 by owner direction — selects
**Option B** (durable session-scoped queue-owner binding) and adopts **Option D**
(the storage/mutation milestone split) in the same decision, unblocking
`plans/implementation/user-prompt-queue/002b-queue-mutation-operations-and-authority.md`
(M002b)

Decision owners: project maintainers

> **Pre-acceptance correction.** While this ADR was still `proposed`, its Decision
> and Compatibility sections stated that the owner binding would be folded into the
> **v80** migration. M002a had already landed v80 with the queue table alone before
> acceptance, and a shipped migration is never edited. Both sections were corrected
> to **v81** at acceptance. This is a factual correction to a layout claim, not a
> change of decision: Option B, its refusal semantics, and its verification
> requirements are exactly as authored. Recorded here rather than silently edited so
> the difference between the proposed draft and the accepted record is visible.

Related specification sections:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#6-canonical-identity-relationships`
- `plans/000-long-term-specification.md#15-read-only-session-observation`
- `plans/000-long-term-specification.md#22-audit-architecture`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/001-terminology-and-domain-model.md`

Affected subsystem roadmap:

- `plans/subsystems/user-prompt-queue-roadmap.md`

Related ADRs:

- `plans/adrs/ADR-0007-shared-session-control-ownership.md` (accepted)
- `plans/adrs/ADR-0035-durable-user-prompt-queue-ownership.md` (accepted 2026-10-18)
- `plans/adrs/ADR-0036-mid-turn-prompt-steering-delivery-contract.md` (accepted 2026-10-18; status label refreshed from `proposed`, no decision content changed)

## Context

ADR-0035 was accepted on 2026-10-18. Its Decision section states:

> Queue mutation is authorized by the same controller predicate that governs turn
> admission, and queue reads are subject to session read authority.

Applied to the current repository, that clause has **no answer** for the case
that matters most.

The durable controller lease is **turn-scoped**. In
`crates/codegg-core/src/session_control.rs`:

- `SessionControllerStore::acquire` writes `session_turn_controller` keyed by
  `(session_id, turn_id)`, and calls `validate_turn_id`, which rejects an empty
  locator.
- `SessionControllerStore::release` executes
  `DELETE FROM session_turn_controller WHERE session_id = ? AND turn_id = ?`.
- `finish_turn_receipt` and `interrupt_incomplete_turn_receipts` end the turn
  and therefore the lease.

So the row exists only while a turn is running. `CoreDaemon::controller_lease_for`
(`src/core/daemon_control.rs:85`) reads the store first and returns `None` when
there is no row. `CoreDaemon::check_turn_controller` (`:124`) then takes its
`not_controller()` arm for **every** principal, including the legitimate owner,
because it cannot reach a lease to compare.

There is exactly one path that passes while idle, and it is a bypass rather than
a predicate: `check_turn_controller` returns `None` early when
`is_controller_bypass(authority)` holds, which is
`authorization::is_local_owner_broad`, i.e. `principal_id ==
LOCAL_OWNER_PRINCIPAL_ID`. That test is **identity-only**. It is not bound to a
session, so it authorizes mutation of *any* session's queue for the local
interactive owner.

The consequence is that ADR-0035's refusal clause — "Mutating the queue of a
session you do not control is refused, not silently applied" — is satisfiable
mid-turn (the lease exists) and unsatisfiable between turns (no lease). Queue
mutation between turns is the primary use: a user opens `/queue` to compose the
next instruction precisely when no turn is running. A predicate that only works
while a turn is active does not authorize the feature it was written to
authorize.

This must be decided before any queue mutation handler is written, because every
option has a different schema, protocol, and compatibility cost, and choosing
after the table ships means migrating again.

## Decision drivers

- ADR-0035's refusal clause must hold **while idle**, not only mid-turn.
- The queue is durable, so its mutation authority must be durable too. An
  in-memory or lease-scoped answer is wrong across daemon restart, which is the
  exact condition the feature exists to survive.
- The answer must not fork, weaken, or bypass the existing turn-admission
  authority. ADR-0007's controller lease keeps its meaning for turns.
- The answer must not let two attached controllers interleave mutations into one
  queue, which is a roadmap invariant ("Queue order is a daemon-owned total
  order"; "Observers and non-controller contributors are refused").
- Queue authority must not change turn-admission semantics: a queued entry
  still becomes a turn only through ordinary `TurnSubmit` (ADR-0035).
- Observers and non-controller contributors must gain no way to author session
  input, and the frontend control must be absent rather than merely ineffective.

## Considered options

### Option A — Lease-or-bypass: reuse `is_controller_bypass` when no lease exists

Mutation requires the turn lease when one is held, and otherwise requires the
local interactive owner.

Benefits: no schema change, no new identity, no migration. Smallest code.

Costs and failure modes: the authority is no longer one predicate — it is a lease
check in one state and an identity string comparison in the other, which is
exactly the "two answers wearing one name" failure. `is_controller_bypass` is not
session-bound, so it cannot express "a session you do not control"; a local owner
can mutate any session's queue, and nothing in the code states that as a
deliberate grant. It also cannot answer for a non-local principal with
`agent.invoke` who is not currently holding a lease, so remote and team
principals can never queue at all — a silent capability hole rather than a
refusal. Adopting it would make the bypass load-bearing for a durable,
user-authored resource, which is the opposite of ADR-0035's intent.

### Option B — Durable session-scoped queue-owner binding

The daemon records, per session, the principal authorized to mutate its queue.
This is a durable row (a new table, or a column on the session record) written at
queue creation and changed only by an explicit, audited transfer. Mutation
requires the caller to be that owner, independent of turn state. Mid-turn, the
turn controller lease and the queue owner are checked **together**, and a
mismatch is refused rather than resolved by precedence.

Benefits: "who may mutate this queue" becomes a durable, session-scoped,
restart-surviving fact; it is expressible while idle; it does not fork or weaken
turn admission; it gives ADR-0035's refusal clause something real to check; and
it makes an owner/controller mismatch an explicit typed refusal instead of an
ordering accident.

Costs and failure modes: a second identity concept adjacent to the controller
lease, which is a real conceptual cost — two notions of "who owns this session"
invite exactly the drift ADR-0035 was written to prevent. It requires a
migration (folded into v80, so no extra version bump), a transfer operation, and
audit coverage. It needs a stated rule for what happens when the queue owner is
not the turn controller, which is the sharp edge of this option.

### Option C — Capability-only: any principal holding `agent.invoke` may mutate

Mutation is authorized by the operation descriptor's capability alone, with no
session-scoped identity at all.

Benefits: smallest policy surface; consistent with how `session_selection_update`
and similar project-scoped operations work.

Costs and failure modes: two attached controllers with `agent.invoke` can both
mutate one queue, so reorders interleave and the "daemon-owned total order"
invariant is only enforced by transaction, not by authority. It directly
contradicts ADR-0035's "same controller predicate" clause, so accepting it is
effectively a silent amendment of an accepted ADR — which requires superseding
ADR-0035, not quietly reinterpreting it. Rejected.

### Option D — Split the milestone and defer the mutation decision

Ship the durable entry model, bounds, expiry, and read projection first, and
leave every mutation operation, its authorization, and its protocol variants to a
later milestone gated on this decision.

Benefits: unblocks real, dependency-clean infrastructure work immediately while
the authority question is settled by a maintainer. It also matches the
infrastructure/capability separation the roadmap already requires: ADR-0035's
M002 is declared infrastructure and "must not be described as a user-visible
queue".

Costs and failure modes: the read projection has no producer, so the queue always
reads empty until the gated milestone lands, which must be stated rather than
implied. Two milestones instead of one.

## Decision

**Option B**, with **Option D adopted in the same decision**.

A durable, session-scoped queue-owner binding is the authority for queue
mutation. It is independent of turn state: it exists while idle, survives daemon
restart, and is changed only by an explicit audited transfer. When a turn is
active, the queue owner and the turn controller are both required to match the
caller; a mismatch is a typed refusal, never a silent precedence rule. This
supplements ADR-0007 and ADR-0035 rather than modifying either: the turn
controller lease keeps its meaning for turns, and ADR-0035's durability and
admission clauses are unchanged.

`is_controller_bypass` is **not** extended to carry queue-mutation authority. It
remains what it is today, and the closure record for the mutation milestone must
record that the queue has a real session-scoped authority rather than a bypass.

**Option D is adopted simultaneously and does not depend on which option the
maintainer finally prefers.** The milestone is split into a storage-and-read
slice that is dependency-clean under every option, and a mutation slice gated on
this ADR's acceptance. If the maintainer selects Option A or C instead, only the
gated slice changes; the storage slice stands.

Under Option B the following are additionally decided:

- The owner binding is recorded in a **v81** additive migration, not folded into
  v80. The storage slice (M002a) landed v80 — the queue table alone — before this
  ADR was accepted, and a shipped migration is never edited afterwards. This
  corrects a layout claim made while the ADR was still `proposed`; the decision
  itself is unchanged, and the correction is stated in full under
  [Compatibility and migration](#compatibility-and-migration).
- Consequently the sequencing is: **M002a → v80 (queue table, bounds, expiry,
  read projection)**, then **M002b → v81 (queue-owner binding, adoption,
  transfer)**. M002b's plan already carried this contingency, so no plan needs
  rewriting to accommodate it.
- A queue with no recorded owner has **no** authorized mutator. It is not
  implicitly owned by the session creator, and it is not implicitly owned by the
  local interactive owner. An unowned queue is an invalid state that only an
  explicit adoption operation can repair, and M002b must state that operation.
- Transfer of queue ownership is an explicit, audited operation with the same
  capability requirement as `session_control_transfer`.

## Consequences

### Positive

- ADR-0035's refusal clause becomes satisfiable between turns, which is where
  queue mutation is actually used.
- The queue's authority is durable, so a daemon restart does not silently widen
  or narrow who may mutate it.
- An owner/controller mismatch during an active turn becomes an explicit typed
  refusal instead of depending on check order.
- The storage slice proceeds now under any option.

### Negative

- Two ownership notions now sit adjacent to each other: the turn controller lease
  and the queue owner. Every future reader must be told which question is being
  asked.
- Owner adoption and transfer are new operations that must be authorized,
  audited, and tested.
- An unowned queue is a real invalid state needing an explicit repair path.

### Neutral or deferred

- Desktop parity is unaffected; the authority is a protocol-level property.
- Whether the queue owner is implicitly seeded from the session creator at queue
  creation is a policy detail of the adoption operation, deferred to M002b and
  requiring an audit decision there.
- Whether ADR-0036's steering path should require the same owner binding is out
  of scope here; `TurnSteer` keeps its existing controller predicate.

## Compatibility and migration

Two additive migrations, in sequence, both already planned:

- **v80** carries the queue table, its bounds `CHECK`s, the covering index, and the
  read projection — milestone M002a. `STORAGE_LAYOUT_VERSION` moves 79 → 80, which
  `scripts/check_project_catalog_invariants.py` and `tests/storage_migrations.rs`
  both assert.
- **v81** carries the queue-owner binding and nothing else — milestone M002b.
  `STORAGE_LAYOUT_VERSION` moves 80 → 81 under the same two assertions.

No existing table is altered, no stored data is rewritten, and no shipped migration
is edited. No new protocol variant is introduced *by this ADR*; the mutation
requests named in ADR-0035 carry the authority, and this ADR decides what checks
them.

Had Option A or C been selected, v81 would not exist: v80 would have gained no
owner column and the storage slice would have proceeded unchanged, with only the
mutation slice affected. That is the property this decision preserves — the
storage slice is unaffected by the choice among A, B, and C.

## Security and reliability implications

**Authorization.** Queue mutation has a session-scoped durable authority rather
than an identity-string bypass. Observers and non-controller contributors remain
refused. Because the queue survives restart, so does its authority: a restart
cannot leave a queue with a wider mutator set than it had before.

**Reliability.** An unowned queue fails closed — no authorized mutator — rather
than open. A transfer racing a mutation must converge on one winner through
transaction, not leave a queue with two owners.

**Observability.** Ownership changes are auditable with entry and owner identity,
never with prompt text. The audit action reuses the existing `prompt_submit`
shape ADR-0035 already selected.

## Verification

- A test proving the mutation authority holds while a session is **idle**, with
  no turn and no lease row — the case that motivated this ADR.
- A test proving a non-owner principal with `agent.invoke` is refused for every
  mutation, idle and mid-turn.
- A test proving an owner/controller mismatch during an active turn is a typed
  refusal naming the mismatch.
- A restart test proving queue ownership survives daemon restart unchanged.
- A transfer test proving two concurrent transfers converge on exactly one
  winner.
- A test proving `is_controller_bypass` remains scoped to its current callers
  and gains no queue authority.

## Supersession

None. This decision supplements accepted ADR-0007 and accepted ADR-0035; it
modifies neither. If it is not accepted, the mutation slice stays blocked and the
storage slice proceeds.