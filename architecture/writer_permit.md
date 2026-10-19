# Checkout Writer Permit — the durable, generation-fenced writer token (M005 corrective C002)

## Purpose

One durable, authoritative answer to "who may write to this managed checkout
right now".

The permit is the **single CodeGG-held writer token for a checkout**. It is
deliberately not:

- a model-visible tool, and not a tool-adjacent capability at all;
- an admission scheduler, or a job-queue slot;
- a project-level global mutex — two different checkouts write in parallel;
- a claim that any particular writer has been mediated.

C002 builds the **authority**. C003 (native file and Git writers) and C004
(non-broker sinks) are what route writers through it. Nothing in this
milestone enables shared mutation, and no user-visible shared-mutation
selector exists.

Roadmap: `plans/subsystems/agent-workstream-blackboard-m005-writer-containment-corrective-addendum.md` (C002).
Implementation plan: `plans/implementation/agent-workstream-m005-writer-containment-corrective/002-durable-writer-permit-and-quarantine.md`.
Closure: `plans/closure/agent-workstream-m005-writer-containment-corrective/002-status.md`.
Decision: `plans/adrs/ADR-0042-restricted-agent-shared-checkout-writer-contract.md` (**accepted 2026-10-19**, option B — restricted CodeGG-managed coordination).
Contract: `plans/implementation/agent-workstream-m005-writer-containment-corrective/c001-restricted-profile-contract.md`.
Writer inventory: `docs/writer-disposition.toml`, guarded by `scripts/check_writer_disposition.py`.

## The permit is not the lease

The two are different, and the difference is the point.

| | Worktree lease | Writer permit |
|---|---|---|
| Question answered | *Who owns this checkout?* | *Who may write to it right now?* |
| Lifetime | Outlives a single write | One critical section |
| Owner | The run that holds the checkout | The writer currently writing |
| Store | `worktree_lease` (`managed_worktree.lease_generation`) | `writer_permit` (one row per worktree) |

A checkout can be owned by one run and written by that run's writer. Because
the two identities are tracked separately, a re-lease does not silently
transfer write authority, and a write does not silently re-lease the checkout.

## Where It Lives

```
crates/codegg-core/src/writer_permit.rs        # the authority: typed scope, state machine,
                                              # durable store, CAS, quarantine, reconciliation
crates/codegg-core/src/storage/immediate.rs     # the shared BEGIN IMMEDIATE write transaction
src/core/writer_permit.rs                      # the daemon adapter: authority resolution,
                                              # bounded per-checkout waiter, the holder guard,
                                              # the operator surface
crates/codegg-core/src/session/schema.rs        # migrate_v84: the writer_permit table
src/core/runtime_deps.rs                       # the daemon owns exactly one permit service
src/core/daemon_construct.rs                   # startup reconciliation of orphaned permits
```

`codegg-core` holds the domain types and the store. It has no dependency on the
TUI, the server, the tools, or the scheduler. The daemon adapter above it
supplies authorization, does the waiting, and owns the holder's release
contract. The split is enforced by `scripts/check-core-boundary.sh`.

## The state machine

```
                acquire
     Available ─────────► Held
         ▲                  │ │
         │                  │ │ begin_release
         │  complete_release│ ▼
         │   (settled)   Releasing
         │                  │
         │                  │ complete_release (unproven)
         │                  ▼
         └────────────── Quarantined
                             ▲
        quarantine (any state) ─┘
```

`Releasing` exists so that a crash *between* the holder declaring it is done
and the permit actually being freed is distinguishable from a holder that is
still working. Both are unsafe; only one of them is *known* to be mid-release.

`Quarantined` is terminal until an operator releases it explicitly. A quarantine
is never cleared by a timer: **a lease deadline is never enough to free a live
writer**, and neither is a restart.

## The fence

Every mutating operation presents three things, and all three must match the
persisted row:

1. the **owner token** — a non-duplicable, identity-bound secret minted at
   acquisition, so "the holder" is never ambiguous;
2. the **revision** — a monotonic counter bumped on every state change, so a
   stale actor cannot act on a row it has not re-read;
3. the **lease generation** — the worktree lease generation the permit was
   granted under, so a holder from a superseded lease cannot mutate the row
   after the checkout was re-leased.

Together these defeat ABA. A row that was released and re-acquired has a new
revision, a new owner token, and — if the lease moved — a new generation, so a
stale actor's request matches none of them.

A permit held under an old generation is not handed to the new generation's
holder: the request is refused with `StaleLeaseGeneration` and the permit
stays with its holder until that holder releases it or an operator parks it. A
checkout that changed hands mid-write is a checkout whose state is unknown.

## Idempotency

An acquisition carries an idempotency key, and the store binds that key to the
scope's fingerprint for the lifetime of the acquisition.

- A retry with the **same** key and the **same** payload returns the existing
  permit. It does not bump the revision and does not mint a second token.
- A retry with the **same** key and an **altered** payload is refused with
  `IdempotencyConflict`.

The binding is scoped to the live acquisition: the key is cleared when the
permit is released or quarantined, and a later acquisition may reuse it.

## Quiescence

A release is only permitted with **positive** evidence that the caller's
effects settled. `QuiescenceEvidence` has exactly three states:

- `Settled` — the caller's transaction committed or rolled back and its
  effects were observed to settle;
- `NoEffect` — the operation produced no effect at all;
- `Unproven(reason)` — the caller cannot prove it.

`Unproven` does not free the permit. It **quarantines** it, with the caller's
reason. There is no fourth state and no path that frees a permit without
evidence.

An operator's release of a quarantined permit is evidence-gated the same way,
and it never touches the checkout: releasing a quarantined permit says "an
operator looked at this checkout and decided it is safe to write again", and
the files are exactly as the previous writer left them.

## The holder contract

A permit is held through `WriterPermitGuard` in the daemon adapter. The guard
is settled **explicitly**:

- `release(evidence)` frees the permit when the caller can prove its effects
  settled;
- `quarantine(reason, detail)` parks it when it cannot;
- **dropping** the guard without settling it quarantines the permit. It never
  frees it.

That last rule is the whole point. A permit abandoned mid-flight describes a
checkout whose state is unknown, and "unknown" is not "free". A forgotten
holder leaves the checkout parked for an operator rather than handing it to the
next writer while the previous writer's effects may still be landing. When the
guard is dropped without a Tokio runtime to spawn the quarantine on, the permit
stays held and the next daemon restart reconciles it into a quarantine — the
fail-safe direction either way.

## The waiter

The adapter provides a bounded wait for a busy permit.

- **Per checkout.** Each checkout has its own FIFO queue. Two checkouts never
  wait on each other, and a busy checkout cannot consume another's capacity.
- **Fair.** Only the head of a checkout's queue may attempt the
  compare-and-swap, so a waiter cannot be starved by later arrivals.
- **Bounded.** The queue depth is capped
  (`MAX_WRITER_PERMIT_WAITERS_PER_WORKTREE`), and a wait has an explicit
  timeout and a cancellation token.
- **Not a second admission queue.** This is the permit's own wait, not a job
  admission path. The scheduler's admission keys cannot carry it: the
  writer-bearing job kinds (`Shell`, `ManagedProcess`, `Test`, `Lint`,
  `Python`, `ToolProgram`) have empty exclusivity sets, and only
  `exclusive:`-prefixed keys conflict at all.

A cancelled or timed-out wait leaks nothing: the waiter holds no token while
it waits, and if the permit is taken in the same instant the wait is
cancelled, the permit is released immediately with `NoEffect` evidence.

## Restart reconciliation

A daemon restart means every previously-held permit is orphaned: the process
that held it is gone. Every `held` or `releasing` permit found at startup is
therefore quarantined, with reason `DaemonRestart`.

Reconciliation **never replays the holder's operation**. Repairing metadata by
re-running a mutation would be a second effect, not a repair; the only safe
repair is to park the permit and let an operator look.

## Authorization

The daemon adapter resolves the scope from authoritative stores, never from a
request payload. `WorktreePermitAuthority` enforces:

1. the worktree exists and is in a state that can be written;
2. the worktree has an active lease — a checkout nobody has leased has no
   writer authority to hand out;
3. the requesting run **holds the lease**. A run that does not own the
   checkout cannot write to it, and a run in another project cannot own this
   checkout's lease, so a cross-project request fails here rather than at the
   store;
4. the group reservation, when one is named, is still live — a group that has
   finished no longer covers this checkout;
5. the principal is authorized for the worktree's project, checked through the
   daemon's authorization service with the `file.modify` capability when one
   is configured.

## Redaction

The operator view (`WriterPermitView`) carries identity and state: the
checkout, the project, the workspace, the repository, the state, the operation
class, the revision, the lease generation, the owner principal and run, and
the quarantine reason.

It never carries the possession token, a path, a command, or any other free
text. The operation is a closed enum, so the permit cannot become a channel
for user content.

## What This Does Not Claim

- **Not a filesystem exclusion.** The permit coordinates CodeGG's own writers.
  An unrelated process running as the same user is outside its guarantee.
- **Not a sandbox.** Nothing here contains a process.
- **Not an enablement.** No writer consults the permit yet. C002 ships the
  authority; C003 and C004 ship the routing.
- **Not a claim of mediation.** A permit existing does not mean any particular
  writer went through it.

## Verification

`crates/codegg-core/tests/writer_permit.rs` (21 tests) covers the store:
N=2 and N=16 contention with exactly one holder, distinct checkouts acquiring
in parallel, stale owner token / stale revision / stale lease generation,
ABA across a release and re-acquire, idempotent retry and altered-payload
retry, unproven quiescence quarantining instead of releasing, the quarantine
being terminal until an operator release, an expired lease not freeing a live
writer, transfer minting a new token and invalidating the old one, restart
reconciliation quarantining live and interrupted permits, a quarantine
surviving a close and reopen, the rollback contract, a panicking holder
leaving the permit held rather than freed, the redacted view, and the store
under competing connections.

`tests/writer_permit_service.rs` (12 tests) covers the adapter: the lease
owner acquiring, a run without the lease being refused (the cross-project
denial), a principal outside the project — a viewer, and a principal with no
membership — being refused (the cross-principal denial), a waiter taking the
permit when it is released without holding anything while it waits, a
cancelled wait and a timed-out wait leaking nothing, distinct checkouts not
waiting on each other, a dropped guard quarantining rather than freeing,
unproven quiescence parking the permit, the operator inspect and release
surface, the operator release needing proven quiescence, and startup
reconciliation parking a permit left live.

The `BEGIN IMMEDIATE` primitive the store depends on is shared with the
user-prompt queue store and is guarded by
`scripts/check_queue_write_transaction_shape.py`, whose self-test proves it
rejects both a deferred read-then-write path and a closing rollback.
