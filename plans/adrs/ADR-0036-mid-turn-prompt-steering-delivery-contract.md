# ADR-0036: Mid-Turn Prompt Steering Delivery Contract

Status: **accepted**

Date: 2026-10-17 (authored); **accepted 2026-10-18**.

**Supersedes nothing.** This ADR fixes the delivery guarantee of mid-turn prompt
steering and selects **Option A**. It authorizes
`plans/implementation/user-prompt-queue/006-*` (M006) — the steering frontend and
send-now action — and binds the vocabulary that M004 and M005 must use when they
render entries alongside queue delivery.

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`
- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/001-terminology-and-domain-model.md`

Affected subsystem roadmap:

- `plans/subsystems/user-prompt-queue-roadmap.md`

Related ADRs:

- `plans/adrs/ADR-0007-shared-session-control-ownership.md` (accepted)
- `plans/adrs/ADR-0035-durable-user-prompt-queue-ownership.md` (accepted 2026-10-18)
- `plans/adrs/ADR-0037-idle-session-queue-mutation-authority.md` (accepted 2026-10-18) —
  raised after ADR-0035's acceptance. **Accepted as written and it does not change
  this ADR**: `TurnSteer` keeps this ADR's controller predicate and does **not**
  adopt a queue-owner binding. ADR-0037 governs *queue mutation between turns*;
  steering governs *injection into a running turn*, which by definition has a live
  controller lease. Had the two been unified, that would have been a supersession
  decision for the maintainer, not an implementation edit.
- `plans/adrs/ADR-0038-queue-delivery-execution-authority.md` (accepted 2026-10-18) —
  governs *as whom* a due entry is submitted. Orthogonal: M003's daemon delivery
  uses no steering channel at all.

## Context

A queue answers "what do I say after this turn finishes". Steering answers a
different question: "interrupt what is happening now". The two have different
delivery guarantees, different failure modes, and different authority, and
conflating them would produce a control that lies to the user.

**The backend already exists and is unreachable.** `CoreRequest::TurnSteer` is
implemented in the daemon's turn dispatcher and forwards text to
`TurnHandle.steer_tx`, a bounded `mpsc::Sender<String>` carried on the active
`TurnHandle`. A search of the TUI source finds no issuer of `TurnSteer`: the
capability has been implemented, authorized, and never wired to a key.

**Its semantics are best-effort, and the word "force" misdescribes them.** The
handler uses `try_send` and distinguishes three failure modes:
`steer_queue_full` when the bounded channel is full, `steer_unavailable` when the
receiver has closed, and `steer_not_supported` when the turn carries no steering
channel at all. It is additionally gated by the same controller predicate as turn
cancellation, so an observer or a non-controller contributor is refused. A full
channel is a routine condition under load, not an exceptional one.

**A second, unrelated mechanism already drains queued prompts into the agent
loop.** `AgentLoop::drain_follow_up` consumes a channel of follow-up prompts with
`try_recv` at turn boundaries, powering goal-continuation turns. Its own
documentation states that late-arriving follow-ups are *not* processed by the same
`run()` call. This is agent-scoped, in-process, boundary-timed, and invisible to
the user; `steer_tx` is turn-scoped, best-effort, and user-initiated. Both are
"a prompt that arrives while the agent is working". Two mechanisms with that
description and different guarantees will drift, and a reader encountering
"queued prompt" in either will reasonably assume the other's semantics.

A design is therefore required for the delivery guarantee of user-initiated
mid-turn injection, for the consumption semantics of a steered queue entry, and
for the terminology that keeps this distinct from the agent-loop drain.

## Decision drivers

- A user-visible control must not promise more than the mechanism delivers. A key
  that can drop the message must not be labelled as one that forces it out.
- The existing, authorized backend should be reused rather than reimplemented; a
  second injection channel would double the surface and the failure modes.
- The distinction between "next turn" and "this turn" must be legible in code,
  documentation, and UI copy, not only in the mind of the person who wrote it.
- A steered message must not be silently lost if the channel is full, and must
  not be silently duplicated if it was delivered.

## Considered options

### Option A — Reuse `TurnSteer` as-is, and state its guarantee honestly

A "send now" action sends `TurnSteer` for the active turn. On `Ack` the message
is delivered into the turn's steering channel. On `steer_queue_full` or
`steer_unavailable` the message remains in the queue, visibly undelivered, and the
user is told why. The action is described as best-effort injection, not forcing.

Benefits: reuses a backend that is already implemented, authorized, and
controller-gated; adds no new injection channel; no new failure semantics to
invent; the retained-on-failure behavior is strictly safer than a drop.

Costs and failure modes: delivery is not guaranteed, so the feature cannot claim
otherwise; a full channel under heavy use is a realistic outcome the UI must
present without alarm; `Ack` means "accepted into the turn's steering channel",
not "the model has read it".

- **Verdict: accepted.** It is the only option that adds no injection surface and
  no new failure semantics to a capability that already exists and is merely
  unreachable.

### Option B — Make steering guaranteed by cancelling and resubmitting

A "send now" action that guarantees the message reaches the next turn by
cancelling the running turn and immediately submitting the message as a new turn.

Benefits: delivery is deterministic and observable.

Costs and failure modes: this changes turn semantics — work in flight is
discarded — which is a cancellation, not a steering. It would also make the same
gesture mean two entirely different things depending on an invisible channel
state, and it interacts with partial in-flight tool effects, worktree ownership,
and the stop-confirmation gesture the status row already implements. The cost is
borne by the user's in-flight work.

- **Verdict: not selected.** It is a cancellation wearing a steering label, and
  the cost lands on in-flight work the user did not ask to discard.

### Option C — Add a durable, ordered per-turn injection channel

Replace the bounded in-memory channel with a queue drained at the turn's next safe
boundary.

Benefits: ordering and unbounded capacity become possible.

Costs and failure modes: it converts "steer into this turn" into "queue for later",
which is the thing ADR-0035 already specifies. It also reintroduces the
deliver-once problem across restart, requiring receipt semantics for injection —
significant new durability surface for a capability that currently exists and is
merely unreachable.

- **Verdict: not selected.** It converts "steer into this turn" into "queue for
  later", which is ADR-0035's job, and buys ordering guarantees with a new
  deliver-once problem across restart.

## Decision

**Accepted 2026-10-18, selecting Option A.** The trade being accepted, stated
plainly: **steering is best-effort and this decision does not change that.** The
user gives up delivery certainty for a reuse of an already-implemented and
already-authorized backend, in exchange for the queue remaining the single durable
home for user-authored text — a message that fails to steer is retained rather
than dropped or silently half-sent. A feature that cannot guarantee delivery is
still worth shipping provided the UI says so; the failure this decision accepts is
a visible, retryable one, not a lost prompt.

**Option A.** User-initiated mid-turn injection uses the existing `CoreRequest::TurnSteer`
path and nothing else. No second injection channel is introduced. No steering
variant cancels or resubmits a turn.

**The guarantee is stated as acceptance, not delivery.** `Ack` means the message
was accepted into the active turn's steering channel. It does not mean the model
has read it, incorporated it, or will not have already acted. User-facing copy,
keymap hints, and documentation use "send now" or "steer" and must not use
"force".

**A failed steer does not consume the message.** On `steer_queue_full`,
`steer_unavailable`, or `steer_not_supported`, the message is retained in the
queue in its original position, marked as not delivered, and the reason is shown.
A steer is idempotent by message identity: retrying the same entry does not create
a duplicate, and an `Ack` followed by a lost connection is reconciled by entry
identity rather than by resubmission.

**Steering authority is turn-controller authority.** Steer is refused for
observers and non-controller contributors, using the existing predicate. A refusal
is distinct from a channel-full failure and must not be reported as one.

**Terminology is fixed and the two mechanisms are distinguished in documentation.**
"Queue" refers only to the durable user prompt queue of ADR-0035, whose entries
are delivered by a subsequent turn. "Steer" refers only to mid-turn injection via
`TurnSteer`. The agent loop's follow-up drain is named and documented as a
distinct, agent-internal, turn-boundary mechanism and is **not** renamed and
**not** extended by this work; a cross-reference between the two is required
wherever either is described, so that neither reader infers the other's semantics
from the shared word "queued".

## Consequences

### Positive

- A reachable, already-authorized backend becomes user-visible with no new
  injection surface and no new delivery semantics.
- Retaining a message on failure is strictly safer than dropping it, and it keeps
  the queue the single durable home for user-authored text.
- Naming the guarantee precisely prevents the UI from promising reliability the
  channel cannot provide.

### Negative

- The feature is best-effort. A user who needs certainty cannot get it from this
  control, and the UI must say so without becoming alarming.
- `steer_queue_full` is reachable under load and is a real error path requiring
  copy, tests, and a non-alarming presentation.
- "Steer" and "queue" both being live at once requires careful UI copy so the two
  are distinguishable at a glance.

### Neutral or deferred

- Whether a successfully steered entry remains visible in the queue after
  delivery is a UX decision for the milestone that implements the box, not a
  contract change here. It MUST NOT be deleted before delivery is acknowledged,
  and MUST NOT be re-sent without an explicit user action. **M004 and M005 must
  not decide this either way** — they ship before steering exists, so an entry
  can only ever leave the queue through M003 delivery, and a UI that quietly
  implies steering already removed an entry would be lying about a capability
  that does not exist.
- Bounding the steering channel capacity, and any fairness policy between steering
  and the turn's own internal follow-ups, remain open and belong to the agent-loop
  side of the seam.
- The WS-attach transport accepts prompt strings only; whether it may carry
  `TurnSteer` at all is a transport question the implementing milestone must
  answer explicitly rather than assume. **Now binding on M006**, not optional:
  the roadmap already forbids routing a TUI answer through `AppMode` alone, and a
  send-now action that silently no-ops on one transport is the same class of bug.

## Compatibility and migration

No storage migration and no new protocol variant. `TurnSteer` already exists in
`codegg-protocol` and in the daemon dispatcher; this decision adds an issuer.
Frontends that do not implement a steer action are unaffected.

One behavioral consequence must be recorded rather than discovered later: today no
frontend issues `TurnSteer`, so the daemon arm is effectively untested by
end-to-end product use. Wiring it makes previously unreachable error branches
(`steer_not_supported`, `steer_unavailable`, `steer_queue_full`) reachable for
the first time, and each must have product-visible behavior before the capability
is considered complete.

## Security and reliability implications

**Authorization.** Steer inherits the existing controller predicate. It must never
be widened to allow any attached principal, and an authorization refusal must not
be reported as a channel condition.

**Secret handling.** Steered text is user-authored prompt content delivered into a
running turn. It must not be logged verbatim; diagnostics use the existing
redaction path.

**Resource use.** The steering channel is bounded. A frontend that offers a
send-now action must not implement a blind retry loop against `steer_queue_full`,
which would turn a bounded channel into a contention source. Retain and let the
user retry.

**Idempotency.** Entry identity, not text equality, decides whether a retry is a
duplicate. Two identical prompts typed by the user are two entries and must not
collapse.

## Verification

- A test proving the action is refused for an observer and for a non-controller
  contributor, with a refusal distinct from channel-full.
- A test proving `Ack` retains the entry and does not schedule a resubmission, and
  that a transport loss after `Ack` does not duplicate delivery.
- Tests for each of `steer_queue_full`, `steer_unavailable`, and
  `steer_not_supported` asserting the message is retained, the reason is
  user-visible, and the entry order is unchanged.
- A test proving two identical entries are distinct and both survive.
- A documentation check asserting that "force" does not appear in steer-related
  user-facing copy, keymap hints, or the architecture docs for this mechanism.
- A cross-reference check that the agent-loop follow-up drain and `TurnSteer` are
  distinguished wherever either is documented.

**Status of this list.** No item above is satisfied yet. Acceptance authorizes
M006; it does not retroactively satisfy a test. The three backend error branches
were unreachable at authoring and remain unreachable until M006 ships an issuer,
so each test above must be written *after* the frontend exists — a test written
against a handler called directly, as the terminal-resume surface did, would
prove the branch and not the feature.

## Supersession

None. This decision does not modify accepted ADR-0007; it applies the controller
predicate that ADR-0007 established to steering delivery.