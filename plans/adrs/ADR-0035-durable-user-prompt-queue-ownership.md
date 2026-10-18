# ADR-0035: Durable User Prompt Queue Ownership and Turn Admission

Status: accepted

Date: 2026-10-17 (authored); accepted 2026-10-18 by owner direction — gates
`plans/implementation/user-prompt-queue/002a-durable-queue-storage-and-read-projection.md`
(M002a) and `002b-queue-mutation-operations-and-authority.md` (M002b, itself
gated on `plans/adrs/ADR-0037-idle-session-queue-mutation-authority.md`), and
every later milestone that mutates or delivers queue entries

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`
- `plans/000-long-term-specification.md#15-read-only-session-observation`
- `plans/000-long-term-specification.md#22-audit-architecture`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/001-terminology-and-domain-model.md`

Affected subsystem roadmap:

- `plans/subsystems/user-prompt-queue-roadmap.md`

Related ADRs:

- `plans/adrs/ADR-0007-shared-session-control-ownership.md` (accepted)
- `plans/adrs/ADR-0036-mid-turn-prompt-steering-delivery-contract.md` (accepted 2026-10-18; status label refreshed from `proposed`, no decision content changed)

## Context

CodeGG serializes one turn per session. A human who types a prompt while a turn is
active currently loses the text: the composer clears before the transport
decision is made, and the daemon's `turn_already_active` refusal path does not
restore it. The user sees `Turn not submitted: turn_already_active` and an empty
input box. This is a text-loss defect, not a missing convenience.

A proposed queue would let that text wait for the turn to end. Deciding *where a
queued prompt lives* is an ownership question with no safe default, because the
repository's existing contracts point in several directions at once:

- **Session attach is not exclusive.** `CoreRequest::SessionAttach` is dispatched
  identically to `SessionLoad`, and `ClientRegistry::attach_session` appends to a
  per-client `Vec<String>` rather than claiming the session. Two clients can be
  attached to one session.
- **Single-writer is the durable controller lease**, not the attachment.
  `SessionControllerRecord` permits at most one active controller row per session,
  and `acquire` returns `Conflict` to any other contender, changing nothing.
- **Turn admission is atomic and receipted.** Admission commits promotion
  consumption, the controller lease, and a `TurnAdmissionReceipt` keyed by the
  envelope `request_id` in a single transaction. A retry reusing that identity
  converges on the existing receipt rather than spawning a second turn.
- **Startup never replays.** Accepted receipts are marked `interrupted` at
  bootstrap and are not re-submitted.
- **The WebSocket TUI transport deliberately makes no queue-delivery claim.**
  `architecture/human_shell.md` records that it "accepts prompt strings only, so
  it also uses an explicitly labeled composer draft and makes no staging or
  acceptance claim from queue delivery".
- **Composer state is memory-only.** The composer draft and the bounded
  `stashed_prompts` recall list are both process-local and lost on exit.

Consequently a frontend-local queue would silently lose every queued prompt on
reconnect, restart, or daemon restart, and — worse — a frontend that "replayed"
its queue on reconnect would bypass the receipt machinery that makes at-most-once
turn submission correct. That is precisely the failure mode the existing
`request_id` + receipt + never-replay design exists to prevent.

A close structural analogue already exists: `human_shell_promotion` is a durable
staged-prompt table with a 24-hour TTL, `pending`/`expired`/`promoted` status, and
binding to a turn through `consumed_turn_id`. It stages a different payload for a
different consumer, but its storage, expiry, and consumption shape is the shape a
user prompt queue needs.

## Decision drivers

- A queued prompt is user-authored input with the same authority and sensitivity
  as a directly submitted turn. Losing it silently is unacceptable; delivering it
  twice is a correctness violation.
- The queue must be correct across frontend reconnect, daemon restart, and multiple
  attached frontends, or it must refuse to exist rather than exist partially.
- The queue must not create a second submission path that bypasses the controller
  lease, the receipt, and the interrupted-no-replay rule.
- Migration cost must be additive and must not invalidate existing storage layouts.
- Observers and non-controller contributors must not gain a way to author session
  input.

## Considered options

### Option A — Frontend-local in-memory queue

The TUI holds a `Vec` of pending prompts and drains it when a turn ends.

Benefits: no migration, no protocol change, no daemon surface, ships in one
milestone.

Costs and failure modes: every queued prompt is lost on reconnect, frontend
restart, or daemon restart, with no record that it existed. Two attached
frontends each hold divergent private queues. A "send the front of the queue when
the turn ends" rule implemented in the frontend has no receipt, so a crash
between "sent" and "recorded" either drops the prompt or duplicates the turn.
This option optimizes for milestone count and produces a feature that is wrong
exactly when the user most needs it — mid-turn, when things are already going
badly enough to type a follow-up.

### Option B — Daemon-owned durable queue admitted through existing turn machinery

A new session-scoped table holds queued prompts with bounded text, explicit
status, optional `run_at_ms` for delayed dispatch, and an expiry. Queue operations
are typed core requests served by the daemon. A queued prompt becomes a real turn
only by going through the *existing* `TurnSubmit` admission path, which already
owns the controller lease, the receipt, and promotion consumption. The frontend
renders a projection of the queue and issues explicit user commands against it.

Benefits: durable across reconnect and restart; visible to any attached frontend
rather than private to one; reuses the admission path that is already proven
correct for at-most-once submission; expiry bounds growth without an operator;
queue authority is decided in one place rather than per frontend.

Costs and failure modes: requires a storage migration and a version bump; adds
daemon surface that must be authorized; requires a real decision about which
principal's authority governs queue mutation when two controllers contend;
delayed dispatch needs a trigger point, which is a design question in its own
right (see ADR-0036 and the roadmap's M007).

### Option C — Reuse the durable job scheduler

Model each queued prompt as a scheduled job via `ScheduleKind::OneShot { run_at }`.

Benefits: reuses an existing durable, restart-recoverable scheduling substrate
with `OverlapPolicy` and `MissedRunPolicy` already defined.

Costs and failure modes: `ScheduleTemplate` materializes a `JobTemplate` into a
`JobId` through the occurrence materializer. A job is agent/work-execution
shaped — it acquires a scheduler permit and runs through an executor — which is
not what a user prompt is. Forcing prompts through it would grant every queued
prompt scheduler admission, workspace execution context, and executor ownership it
has no claim to, and would couple a text box to the global scheduler's fairness
and concurrency policy. Reusing the job *storage and trigger machinery* is
reasonable and is what the roadmap proposes; reusing the job *execution model* is
not.

### Option D — Reuse `human_shell_promotion` directly

Benefits: a durable staged-prompt table already exists with TTL and status.

Costs and failure modes: that table is bound to human-shell execution: its
consumers are the shell ask/include staging paths, it is consumed at turn
admission by promotion id, and its lifecycle vocabulary (`promoted`, consumed via
`consumed_turn_id`) encodes shell staging. A general user prompt queue has a
different lifecycle (edited, reordered, deleted while pending), different
consumers (TUI and later desktop), and a different authority question. Forcing
both into one table would make the schema a union of two unrelated lifecycles and
would couple prompt-queue retention to shell staging retention.

## Decision

**Option B.** The queue is a daemon-owned, session-scoped, durable resource. The
daemon is the only authority over its contents and ordering. Frontends — the TUI
first, desktop later — render a projection of it and issue explicit, typed,
authorized commands. No frontend may maintain a private authoritative copy, and no
frontend may convert a queue entry into a turn by any path other than ordinary
`TurnSubmit`.

A queued prompt reaches execution **only** by being submitted through the existing
turn admission path. The queue does not introduce a parallel submission route, does
not mint its own receipt namespace, and does not relax the interrupted-no-replay
rule. If a queued prompt cannot be admitted as a normal turn, it stays queued or
becomes visibly failed — it is never force-submitted.

Queue mutation is authorized by the same controller predicate that governs turn
admission, and queue reads are subject to session read authority. Observers and
non-controller contributors may not mutate the queue. Mutating the queue of a
session you do not control is refused, not silently applied.

## Consequences

### Positive

- A queued prompt survives frontend reconnect, frontend restart, and daemon
  restart, which is the property that makes the feature trustworthy mid-turn.
- At-most-once submission semantics are inherited rather than reimplemented.
- The queue is visible to every attached frontend instead of being private to the
  one that typed it.
- Storage growth is bounded by an expiry, so the feature cannot become an
  unbounded retention surface.

### Negative

- A storage migration and a `STORAGE_LAYOUT_VERSION` bump are required, which is
  real cost and a compatibility obligation.
- More daemon surface must be authorized, audited, and tested than a frontend
  change would.
- A queued prompt is now visible to a second attached frontend, which is a
  deliberate information-exposure change relative to a private list.
- Delayed dispatch requires a trigger point and therefore a decision about which
  existing evaluation point owns time-based queue advancement.

### Neutral or deferred

- Desktop parity is a separate milestone; until it lands, desktop has no queue UI
  and the queue is TUI-addressable only. That is a visible gap, not a claim of
  parity.
- Whether a steered entry is removed from the queue or retained is specified in
  ADR-0036, not here.
- Per-entry timers are specified by the roadmap, but the interaction between
  reordering and timer ordering is a UX decision reserved to the milestone that
  introduces both.

## Compatibility and migration

The queue is introduced by one **additive** migration. The current storage layout
is migration **v79** with `STORAGE_LAYOUT_VERSION = 79`; this decision requires a
**v80** migration and a matching constant bump. The repository asserts that
equality in `scripts/check_project_catalog_invariants.py` and
`tests/storage_migrations.rs`, so the bump is a checked obligation rather than a
comment. Additive `IF NOT EXISTS` table creation is the established low-risk
precedent, used for the session-control tables in migration v65.

No existing table changes. No stored data is rewritten. Frontends that do not
implement a queue are unaffected: an absent queue projection reads as an empty
queue. Protocol additions are new `CoreRequest`/`CoreResponse` variants, which is
additive on the wire; clients that predate them never issue them.

## Security and reliability implications

**Authorization.** Queue mutation reuses the turn-controller predicate. An
observer — a read-only attached principal — cannot add, edit, reorder, delete, or
dispatch queue entries. The `/queue` command and its dialog must be registered in
the observer-blocked command list so the control is not merely ineffective but
absent.

**Input handling.** Queued text is user-authored prompt content and is stored as
such. It is bounded per entry and bounded in count, and it must never be written to
logs, receipts, or error messages verbatim. It is redacted or truncated in
diagnostics using the existing redaction path rather than a queue-specific one.

**Retention.** Entries expire. An unbounded queue is a disk-growth and
denial-of-service surface, so a maximum entry count and a maximum text length are
part of the contract, and exceeding them is a typed refusal rather than a
truncation that silently changes meaning.

**Recovery.** On daemon restart, an entry that was mid-admission is reconciled by
the existing receipt status rather than by queue-specific logic. Queued entries
that were never admitted remain queued. Nothing in the queue is replayed
automatically.

**Contention.** Two controllers cannot both mutate one session's queue; the loser
observes the same `Conflict` the lease already returns. Queue ordering is a
total order owned by the daemon, so two frontends cannot interleave reorders into
an inconsistent sequence.

## Verification

- A migration test asserting v79 pools migrate to v80 and that
  `STORAGE_LAYOUT_VERSION` equals the highest migration.
- Round-trip tests for every queue operation, including edit-in-place and reorder,
  proving each is atomic and that reorder is a total order rather than a
  last-writer-wins patch.
- A reconnect test proving a queued entry survives frontend detach/reattach.
- A daemon-restart test proving a queued entry survives and that an
  entry interrupted mid-admission is reconciled by receipt status and never
  double-submitted.
- An authorization test proving an observer and a non-controller contributor are
  refused for every mutating queue operation.
- A bound test proving entry count and text length limits are enforced as typed
  refusals.
- A proof that queued entries reach execution only through ordinary `TurnSubmit`
  admission, with the controller lease and receipt observed.

## Supersession

None. This decision does not modify accepted ADR-0007; it applies the controller
lease that ADR-0007 and its M004 closure established to a new resource.