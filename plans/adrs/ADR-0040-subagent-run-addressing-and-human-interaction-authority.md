# ADR-0040: Subagent Run Addressing and Human Interaction Authority

Status: proposed

Date: 2026-10-18

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/001-terminology-and-domain-model.md`

Affected subsystem roadmap:

- `plans/subsystems/tui-subagent-and-job-visibility-convergence-corrective-addendum.md`
  (milestone **C007**, send-to-subagent)

Related ADRs:

- `plans/adrs/ADR-0036-mid-turn-prompt-steering-delivery-contract.md` (accepted
  2026-10-18) — fixes what `TurnSteer` means and what it guarantees. This ADR
  does not relax any part of it; it decides **whom a human can address**, not
  how an addressed turn is steered.
- `plans/adrs/ADR-0007-shared-session-control-ownership.md` (accepted) — owns
  who may cancel or interrupt a turn.
- `plans/adrs/ADR-0037-idle-session-queue-mutation-authority.md` (accepted
  2026-10-18) — governs queue mutation *between* turns. A subagent reply aimed
  at a finished parent turn falls under this ADR's concern, not under ADR-0036.

## Context

Milestones C001–C006 of the convergence addendum make a delegated run
*visible*: named, listed in the tasks overlay and the `/sessions` surface, and
readable as a tab in the main buffer. None of them make it **addressable**. The
user's original request included "we should be able to interact with it that way
as well", and that half is the one milestone the roadmap could not commit to.

The blocker is concrete, not a matter of effort.

**There is no send path to a subagent.** `CoreRequest::TurnSteer`
(`crates/codegg-protocol/src/core.rs:2482`) is implemented, authorized
(`crates/codegg-core/src/authorization/policy.rs:442`), dispatched
(`src/core/daemon.rs:3302`), and tested — and has **zero issuers under
`src/tui/`**. Its only callers are integration tests. It is keyed on
`(session_id, turn_id)`: a *session turn*, not a delegated run.

**`InputTarget` has two variants.** `Build` (the default composer target) and
`Chat` (`src/tui/panels/mod.rs:1075-1083`). There is no "this message is
addressed to that subagent" concept anywhere in the TUI, so the composer has no
way to express one even if the daemon could accept it.

**A subagent run has two identifiers and neither is currently used to address
it.** `SubagentRow` carries `task_id` — a daemon-local `u64` rendered in decimal
— and `run_id`, the durable UUID now carried verbatim from
`CoreEvent::SubagentStarted`. The parent's roadmap records that an earlier
attempt to join these two against `AgentRunSummary::task_id` could never match
(`architecture/tui.md`, corrected in C001); the field survived only because it
started arriving in the event. So the durable identifier exists and is correct;
nothing has ever routed on it.

**Steering is already a governed operation, and this ADR must not quietly widen
it.** Per ADR-0036, `TurnSteer` is best-effort (`try_send` on a bounded
channel), reports three distinct failure modes (`steer_queue_full`,
`steer_unavailable`, `steer_not_supported`), and is gated by the same
**controller predicate** as turn cancellation. Any design that lets a human
reach a delegated run through `TurnSteer` inherits that predicate — and a
delegated run is not a session turn, so the predicate has nothing meaningful to
evaluate against it.

**Interacting with a delegated run is an authorization question, not a routing
one.** A subagent executes tools under the grant its parent turn already holds,
possibly inside an isolated child worktree under `child_git_policy`. A human
"reply" that steers such a run is a third party directing work that will spend
someone else's authority. Who is the actor, and whose authority is spent, must be
decided before a key is bound — not discovered after it ships.

## Decision drivers

- **No authority escalation.** Addressing a subagent must not be a way to obtain
  capabilities the controlling human does not hold in that session.
- **A reply must not fork a parent turn silently.** Re-entering the parent is
  the user's mental model; forking underneath them is a data-model surprise.
- **Addressing is id-keyed, never index-keyed.** A finished job or a dropped
  subagent must not cause focus to land on a different entity — the parent's
  recorded defect.
- **Cancellation stays separate.** I5: a tab cancels nothing. Interaction must
  not become cancellation by another name.
- **The answer must be honest about failure.** ADR-0036's three failure modes
  exist because a control that silently swallows input is worse than no control.

## Considered options

### Option A — Address the subagent through its controlling parent turn

A human message aimed at a subagent is delivered to the *parent* turn that
dispatched it, carrying the subagent's name as an explicit addressee. The
parent agent decides whether and how to relay it. The TUI gains no new daemon
surface: it routes an already-authorized `TurnSteer`.

Benefits:

- **No new authority.** The reply is delivered under exactly the parent turn's
  existing controller predicate and grant. Nothing that was refused before
  becomes reachable.
- **No new delivery contract.** ADR-0036 already specifies the guarantee, the
  failure modes, and the controller gate; this option reuses all three
  verbatim.
- **Consistent with the mental model.** The user already knows the main agent
  is the thing they talk to; a subagent tab becoming a second conversation
  would be the surprising part.
- **Cheap to un-satisfy later.** If direct addressing is wanted, it can be added
  without undoing this.

Costs:

- **Round trip.** The parent must be running and must relay. A subagent that has
  already finished cannot be reached this way.
- **Best-effort delivery.** A full or closed steering channel is reported, not
  retried — the user must be told, or the feature lies.
- **Indirect.** The parent may decline to relay. The UI must not present the
  reply as delivered to the subagent when it was delivered to the parent.

Failure modes: a finished parent turn (nothing to steer), a parent that ignores
the addressee, and a name collision after a name lease is released.

### Option B — First-class subagent run addressing

A new `CoreRequest::SubagentSteer { run_id, text }`, a per-run steering channel
on the subagent's execution context, a new `OperationDescriptor` in the
authorization policy, and a third `InputTarget`.

Benefits:

- Direct, precise, and it matches what the UI draws.
- Reaches a run the parent is no longer actively steering.

Costs:

- **Net-new daemon surface and a second steering contract** alongside ADR-0036,
  with its own failure modes, its own controller gate, and its own audit
  entries.
- **Couples to run lifetime.** A run that is finishing, cancelled, or already
  reaped leaves a channel with no receiver; the UI must distinguish "not
  reachable" from "not listening".
- **Requires an answer to the authority question this ADR exists to ask**, and
  it is a harder one: the run's authority belongs to the parent turn's grant, so
  a human reply either reuses that grant (and is therefore constrained by
  whatever constrained the parent) or needs its own.

Failure modes: a reaped run id, a run that never opened a channel, and a reply
that outlives the worktree it was aimed at.

### Option C — No interaction; subagent tabs stay read-only

Benefits: zero new authority, zero new daemon surface, and the read-only tab is
already honest about what it is.

Costs: it does not deliver what the user asked for, and it leaves C007 permanently
blocked on indecision rather than on a decision.

## Decision

**Select Option A. Do not build Option B yet.**

A human reply aimed at a subagent is delivered to the **controlling parent turn**
as a `TurnSteer` carrying the subagent's `display_name` as an explicit
addressee. The TUI expresses the addressee in the composer and shows, plainly,
that the message was routed to the controlling agent rather than delivered to
the subagent directly.

Option B is **deferred**, not rejected. It becomes admissible only when all of
the following are true and recorded:

1. A measured use case exists that Option A provably cannot serve — most
   plausibly reaching a subagent whose parent turn has already finished.
2. The authority question has an answer: whose grant a human-directed delegated
   run spends, and what a run may do when its parent is no longer live.
3. A delivery contract for per-run steering exists as its own ADR, or ADR-0036
   is explicitly extended rather than shadowed.
4. Run lifetime is settled, including what a reply to an already-reaped run id
   means.

Ownership: the TUI owns *composing and displaying* an addressed reply. The
daemon owns delivery and the controller predicate, unchanged from ADR-0036.
The subagent executor owns nothing new.

Binding is explicitly **out of scope for this ADR**: no key is bound by it. The
`Shift+X` close, `Ctrl+T`, and the tab-cycling chords keep their current
meanings, and I5 stands — addressing a run never cancels it.

## Consequences

### Positive

- The user can talk to a running delegation through a control whose authority is
  already established and audited.
- One steering contract (ADR-0036) rather than two.
- The read-only-tab honesty is preserved: nothing claims to deliver to a run that
  is not listening.

### Negative

- A subagent whose parent turn has ended is not reachable. This is a real gap,
  and it is the gap Option A is chosen to tolerate until evidence says otherwise.
- Delivery is best-effort and relay is the parent's judgement, so the UI carries
  more explanatory text than a direct channel would.

### Neutral or deferred

- `InputTarget` does not grow a variant under Option A; the addressee is data,
  not a focus mode.
- Cancellation remains exactly where ADR-0007 put it.
- No protocol, storage, or configuration change.

## Compatibility and migration

None. No storage migration, no protocol field, no configuration key. A session
recorded before this decision replays identically.

`STORAGE_LAYOUT_VERSION` is untouched.

## Security and reliability implications

- **Authorization is unchanged.** A reply is delivered under the parent turn's
  existing controller predicate; nothing that was refused becomes reachable.
- **Audit.** The steered text is logged under the same operation as any other
  `TurnSteer`. The addressee is part of the payload, not a new authority class.
- **Isolation.** A subagent in an isolated child worktree is unaffected: the
  reply changes what the agent is asked to do, never where it may write.
- **Denial of service.** The steering channel stays bounded and `try_send`, so a
  reply cannot block the dispatcher. Its three ADR-0036 failure modes remain
  user-visible.
- **Contention.** Two replies racing a finishing turn resolve exactly as they do
  today for a parent turn — one is delivered, the other reports
  `steer_unavailable`.

## Verification

An implementation of Option A is conformant only when:

1. A composed reply addressed to a named subagent is observed reaching
   `TurnHandle::steer_tx` for the controlling turn.
2. Each of ADR-0036's three failure modes is reported to the user rather than
   swallowed.
3. The controller predicate refuses a non-controller exactly as it does for an
   unaddressed steer.
4. A subagent tab remains read-only until this ADR is accepted, and closing a
   tab still cancels nothing (I5).
5. `scripts/check_execution_ownership.py` and
   `scripts/check_scheduler_bypass.py` remain green and
   `docs/execution-ownership.toml` is unchanged — a reply is not a new execution
   surface.

## Supersession

None. This ADR narrows nothing in ADR-0036; it decides the addressing question
that ADR-0036 explicitly left to "who is speaking to whom".

Until this ADR moves to **accepted**, milestone C007 stays blocked and C001–C006
must not imply that interacting with a subagent is possible.