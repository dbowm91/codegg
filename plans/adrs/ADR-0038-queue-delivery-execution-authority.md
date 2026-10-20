# ADR-0038: Queue Delivery Execution Authority and Attributed Admission

Status: **accepted**

Date: 2026-10-18 (authored and accepted).

**Supersedes nothing.** This ADR adds the decision ADR-0035 and ADR-0037 each
left open — **as whom** a queued entry is submitted — and selects **Option B**.
It authorizes
`plans/implementation/user-prompt-queue/003-queue-delivery-through-turn-admission.md`
(M003).

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#6-canonical-identity-relationships`
- `plans/000-long-term-specification.md#22-audit-architecture`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/001-terminology-and-domain-model.md`

Affected subsystem roadmap:

- `plans/subsystems/user-prompt-queue-roadmap.md`

Related ADRs:

- `plans/adrs/ADR-0007-shared-session-control-ownership.md` (accepted)
- `plans/adrs/ADR-0031-authenticated-idle-continuation-delegated-turn-admission.md`
  (accepted)
- `plans/adrs/ADR-0035-durable-user-prompt-queue-ownership.md` (accepted 2026-10-18)
- `plans/adrs/ADR-0037-idle-session-queue-mutation-authority.md` (accepted 2026-10-18)

## Context

ADR-0035 decided **that** a queued prompt reaches execution only through
ordinary `TurnSubmit` admission, and ADR-0037 decided **who may mutate** a queue
while its session is idle. Neither decided **as whom a queued entry is
submitted** once it becomes due. M003 is the milestone that must answer that,
and it cannot be written against a guess.

Three facts make it unanswerable by inspection.

**The daemon may originate an admission, but only through a narrow, guarded,
attributed entry.** The repository already has exactly one such path:
`CoreDaemon::submit_idle_continuation_turn` (`src/core/daemon_continuation.rs:408`).
ADR-0031 restricted it to **exactly two call sites**, enforced by
`scripts/check_idle_continuation_callsites.py`, and its module header states the
reason: any additional caller would be a "general daemon-may-submit-as-anyone
backdoor" (`scripts/check_idle_continuation_callsites.py:12-13`).

**That path reconstructs authority from durable attribution, never by synthesis.**
It loads an `OriginAttribution` row and calls
`AuthenticatedPrincipal::reconstructed(origin_principal, origin_kind, auth_method,
transport_class, delegated_client)` (`src/core/daemon_continuation.rs:468-494`).
It then **diverges if** `origin_principal != grant.owner_principal`, failing
closed as an anti-spoofing invariant (`:480-486`), and re-authorizes against
**current** team state so a revoked, suspended, or narrowed principal fails at
delivery time even if it held authority when the grant was made (`:515-539`).

**The queue-owner row cannot supply those five fields.**
`UserPromptQueueOwnerRecord`
(`crates/codegg-core/src/session/user_prompt_queue.rs:188`) is
`{ session_id, owner_principal, revision, adopted_at_ms, updated_at_ms }` — a
**bare principal id**. `reconstructed` requires `principal_id`, `kind`,
`auth_method`, `transport_class`, and `client_id`, and its own doc comment
requires that "callers must supply fields read from the durable store, never
from untrusted DTOs" (`crates/codegg-core/src/transport_auth.rs:219-220`).
Four of the five fields have no durable source in the owner row. v81 is shipped
and is never edited, so the gap cannot be closed by amending it.

This is not a cosmetic gap. Delivering as the wrong principal would either
(a) acquire the turn controller lease under a synthetic daemon identity, so every
subsequent mid-turn queue mutation by the real owner fails
`user_prompt_queue_controller_mismatch` — a queue that locks itself out the
moment it starts working — or (b) use `is_controller_bypass`, which ADR-0037
explicitly refuses as queue authority because it is an identity-only comparison
not bound to a session.

A further consequence is a **migration-number collision**. ADR-0037's
pre-acceptance correction moved the owner binding out of v80 into v81, leaving
`run_at_ms` for M007 as **v82** (recorded in `plans/registry.md` and
`architecture/session.md`). Any option here that requires a new migration
competes with M007 for v82, and whichever loses moves to v83. That is a
scheduling fact the maintainer should see while choosing, not after.

## Decision drivers

- **At-most-once delivery across restart.** ADR-0035 requires it; the receipt
  mechanism can provide it only if the submission identity is **deterministic**
  from the entry, so a retry re-derives the same receipt and receives
  `TurnAdmissionOutcome::Duplicate` rather than starting a second turn.
- **No new admission route.** ADR-0035 forbids a parallel submission path. The
  queue must reach `handle_turns_request(CoreRequest::TurnSubmit { .. })`, not
  a shortcut around it.
- **No backdoor.** Whatever entry is created must be named, narrow, and
  guarded the way ADR-0031's is, because the failure mode is a daemon that can
  submit as any principal.
- **Fail closed on a revoked owner.** Authorization is re-evaluated at delivery
  time, not inherited from adoption time.
- **No prompt text in audit, receipts, or diagnostics** — including no digest of
  it, per M002b's settled decision.
- **Ownership of a contested migration number** must be an explicit maintainer
  call, not an accident of ordering.

## Considered options

### Option A — Reuse `submit_idle_continuation_turn` and its attribution store

Extend the ADR-0031 entry to carry queue deliveries, keyed by an
`OriginAttribution` row written when the queue owner is adopted or transferred.

- **For:** reuses a guarded, two-call-site, already-reviewed path; attribution is
  genuinely durable and already reconciled at startup; no new migration.
- **Against:** the continuation entry's contract is specifically about idle
  completion hints and its grant model, so widening it mixes two ownership
  domains under one guard; adoption already shipped in M002b **without** writing
  attribution, so every pre-existing queue needs a backfill; and widening a
  guard whose whole purpose is narrowness is the wrong direction.
- **Verdict:** rejected. It trades a narrow guard for a wide one.

### Option B — A named `submit_queued_prompt_turn` entry, attribution resolved from the owner row plus team state

A second daemon-internal admission entry, restricted by its own callsite guard
to exactly two callers — the post-terminal wake and the bounded restart twin —
mirroring ADR-0031. `PrincipalKind` is resolved durably from `TeamStore` by
principal id (it is a stored principal property); `AuthMethod` and
`TransportClass` are bound to a daemon-owned delivery classification rather
than a live connection, because a queue delivery has no originating connection;
and `client_id` is a fixed daemon-owned string. Authorization is re-evaluated
against current team state at delivery time.

- **For:** no migration, so **no v82/v83 collision**; the guard stays narrow and
  auditable; attribution is reconstructed from durable rows only; matches how
  `PrincipalKind` is already stored.
- **Against:** `AuthMethod`/`TransportClass` become a delivery classification
  rather than a record of what really authenticated the original caller, which
  is a small loss of forensic fidelity in audit; and the owner row alone no
  longer fully determines the delivery principal.
- **Verdict:** **accepted**, 2026-10-18. No migration; no v82 collision; the
  guard stays narrow and auditable; attribution is reconstructed from durable
  rows only.

### Option C — A v82 migration adding attribution columns to `user_prompt_queue_owner`

`ALTER TABLE … ADD COLUMN` for `owner_kind`, `owner_auth_method`, and
`owner_transport_class`, written at adoption and transfer, so the owner row
determines the delivery principal completely.

- **For:** strongest fidelity — the row is self-contained, and `reconstructed`
  gets every field from one durable place.
- **Against:** **contests v82 with M007's `run_at_ms`**, so one of them becomes
  v83; the columns are written by M002b code that is already closed and would
  need a backfill for existing owner rows; and it stores transport binding facts
  that belong to a connection, not to a durable ownership grant, so they can go
  stale relative to how the owner actually reconnects.
- **Verdict:** **not selected.** Viable, but rejected in favour of Option B
  because self-contained owner rows cost a migration, a backfill for rows M002b
  already wrote, and the v82 contest — all to record transport binding facts
  that belong to a connection and would go stale against how the owner actually
  reconnects.

### Option D — Deliver under the daemon's own identity

Submit with a daemon-owned principal and let the queue owner hold authority only
for mutation.

- **For:** no attribution question at all.
- **Against:** rejected on correctness, not preference. The turn controller lease
  would be held by the daemon identity, so the owner's next mid-turn queue
  mutation fails `user_prompt_queue_controller_mismatch`. It also makes the
  receipt's `principal_id` name something that is not a user, which is exactly
  the "general daemon-may-submit-as-anyone backdoor" ADR-0031's guard exists to
  prevent.
- **Verdict:** rejected.

### Option E — Keep ADR-0035's model: the user resubmits, nothing auto-delivers

Do not deliver automatically; let the frontend pull the next entry and submit
it as an ordinary client request.

- **For:** no daemon-internal admission at all, so no attribution decision and
  no backdoor risk.
- **Against:** it contradicts ADR-0035's accepted delivery model, and it makes
  delivery depend on a frontend being attached and polling — so a queued prompt
  is not delivered at all when nobody is watching, which defeats the durability
  property ADR-0035 exists to provide.
- **Verdict:** rejected as a reversal of an accepted ADR; recorded so the
  tradeoff is visible.

## Decision

**Option B is selected.** A queued entry is delivered through a named
daemon-internal admission entry, `CoreDaemon::submit_queued_prompt_turn`, whose
delivery principal is reconstructed from durable rows only: the queue owner row
supplies the principal id, `TeamStore` supplies `PrincipalKind` (a stored
principal property), and `AuthMethod`/`TransportClass` are a daemon-owned
delivery classification bound to the durable queue rather than to a live
connection, with a fixed daemon-owned `client_id`. Authorization is
re-authorized against current team state at delivery time.

**The v82 contest is resolved in M007's favour.** Option B needs no migration,
so `run_at_ms` remains **v82** for M007 exactly as recorded in
`architecture/session.md` and `plans/registry.md`. Neither this ADR nor M003
takes a migration number.

The following are settled by this decision:

1. A queued entry is submitted **only** through
   `handle_turns_request(CoreRequest::TurnSubmit { .. })` — the same call the
   `SessionPromptSubmit` precedent makes (`src/core/daemon_turns.rs:831-849`).
   No parallel route, no new receipt namespace, no relaxation of
   interrupted-no-replay.
2. The submission identity is **deterministic from the entry id**, so a retry
   after restart re-derives the same `turn_admission_receipt` row and receives
   `Duplicate` instead of starting a second turn. This is the at-most-once fence
   and it inherits from the receipt mechanism rather than reimplementing it.
3. Delivery is a **named daemon-internal entry** restricted to exactly two call
   sites by a **callsite guard of its own**, in the shape of
   `scripts/check_idle_continuation_callsites.py`. Widening the ADR-0031 entry
   is rejected (Option A).
4. Authority is **re-authorized against current team state at delivery time**.
   A revoked, suspended, or capability-narrowed owner fails delivery and its
   entry is retained with a recorded reason rather than force-submitted.
5. The delivered principal is the **queue owner**, never a synthetic identity
   and never `is_controller_bypass`.
6. **ADR-0036 remains `proposed`** and still gates M006 and M007's copy
   boundary. Nothing here touches it.
   *(Status annotation, added 2026-10-18 without altering the clause above,
   which was true at this ADR's acceptance: ADR-0036 was subsequently
   **accepted, Option A**, on the same day. M006 and M007's copy gates are
   discharged; both are now blocked only on M004 closure. ADR-0038's decision
   is unchanged, and it still does not touch steering.)*

### Option B's accepted trade

`AuthMethod` and `TransportClass` are a **delivery classification**, not a
record of what originally authenticated the owner. A queue delivery has no
originating connection — the owner may have been adopted over one connection and
delivered over none — so a per-connection classification would be a fabrication.
The accepted cost is reduced forensic fidelity in audit: an audit record says
how the daemon delivered, not how the owner authenticated when they queued the
prompt. The entry id, delivering principal, turn id, and outcome are still
recorded, and no prompt text or digest is recorded under any option.

## Consequences

### Positive

- Delivery keeps the durability property ADR-0035 bought, instead of depending
  on an attached frontend.
- At-most-once is inherited from the existing receipt rather than reinvented,
  so a restart mid-admission reconciles by receipt status.
- The daemon's ability to submit as a principal stays narrow, named, and
  statically guarded — the same posture ADR-0031 established.
- A revoked owner stops receiving queued prompts immediately, rather than at
  their next reconnect.

### Negative

- Option B gives up exact forensic fidelity on `AuthMethod`/`TransportClass`:
  the delivery classification describes how the daemon submitted, not what
  originally authenticated the owner.
- M003 adds a daemon-initiated state change, so the queue projection needs a
  push path it does not currently have (see the plan's §3).
- M003 introduces the first `delivering` rows, which requires repairing three
  `status = 'pending'`-only projection behaviours that are correct today only
  because `delivering` has no producer.

### Neutral or deferred

- **M007's `run_at_ms` remains v82.** Accepting Option B removed the contest
  rather than deciding it; M003 takes no migration number.
- Whether a delivered entry is removed from the queue or retained as a terminal
  row is a **presentation** question for M004, not a delivery decision; the
  `delivered` status exists either way.
- Delayed delivery remains M007 and is untouched.
- Desktop parity remains M008.

## Compatibility and migration

- No existing table is rewritten.
- **No migration is required.** Option B reads `TeamStore` for `PrincipalKind`
  and synthesizes only the delivery classification, so nothing is written to the
  `user_prompt_queue_owner` row and v81 stays as shipped. `STORAGE_LAYOUT_VERSION`
  is unchanged by this ADR.
- **The v82 collision does not arise.** `run_at_ms` stays v82 for M007 as
  recorded in `architecture/session.md` and `plans/registry.md`; neither surface
  changes because of this decision.
- An existing queue owner row written under M002b needs no backfill: its
  `owner_principal` is resolved against team state at delivery time.
- No protocol variant changes shape. Additions are new variants, additive on the
  wire.
- An absent queue projection still reads as an empty queue.

## Security and reliability implications

**Authorization.** The delivery operation needs its own
`OperationDescriptor` (`ScopeKind::ViaSession` +
`Capability::AgentInvoke`, matching `session_control_transfer`) and its own
`INSTRUMENTED_OPERATIONS` entry mapping to `prompt_submit`, consistent with
ADR-0037's decision for the six mutations. `scripts/check_audit_coverage.py`
fails a mutating-capability operation in neither list.

**Audit.** A delivery record names the entry id, the delivering principal, the
turn id, and the outcome. It carries **no prompt text and no digest of it** —
only a byte count, matching M002b's settled decision that a length is bounded
while a digest is a content-correlation oracle.

**No force-submission.** If a queued entry cannot be admitted as a normal turn,
it stays queued or becomes visibly failed with a recorded reason
(`failure_reason` already exists in v80). It is never pushed around a refusal.

**Stranded-state reconciliation.** M003 introduces the first `delivering` rows.
A crash between claiming an entry and recording its outcome would otherwise
strand an entry that is invisible to the pending projection, uncounted by the
pending capacity bound, and never retried. Startup must reconcile `delivering`
rows by receipt status. This is a correctness requirement, not polish.

**Fencing.** Two frontends, or a frontend and the restart twin, must not
double-deliver one entry. The deterministic submission identity plus the
`pending → delivering` claim under transaction is what prevents it.

## Verification

- A test proving a queued entry reaches execution **only** through ordinary
  `TurnSubmit`, with the controller lease and the receipt observed.
- At-most-once across daemon restart: an entry interrupted mid-admission is
  reconciled by receipt status and never double-submitted.
- A restart-reconcile test proving a stranded `delivering` row is resolved by
  receipt status rather than left invisible.
- Authorization tests proving a **revoked** owner fails delivery and retains the
  entry, and that a non-owner cannot cause delivery.
- A determinism test proving the submission id is a pure function of the entry
  id.
- A callsite-guard test proving the delivery entry has exactly its two permitted
  callers, and a `--self-test` for the guard itself.
- An ordering test proving delivery follows the daemon-owned ordinal.
- Audit tests proving entry identity and outcome are recorded and **no prompt
  text or digest** appears.
- Guard, clippy, and `scripts/verify.sh quick` results.

Evidence for each lives in
`plans/closure/user-prompt-queue/003-status.md`. Where a listed item is not yet
covered, that record says so explicitly rather than leaving the requirement to
read as satisfied.

## Supersession

None. This ADR does not modify accepted ADR-0007, ADR-0031, ADR-0035, or
ADR-0037. It resolves a question they each left open: **as whom** a queued entry
is submitted.