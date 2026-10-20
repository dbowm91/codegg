# Checkout Writer Permit — the durable, generation-fenced writer token (M005 corrective C002, C003)

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
Decision: `plans/adrs/ADR-0044-restricted-agent-shared-checkout-writer-contract.md` (**accepted 2026-10-19**, option B — restricted CodeGG-managed coordination).
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
src/tool/writer_fence.rs                       # the tool-facing fence contract (C003): the scope,
                                              # the effect classes, the typed refusals
src/core/writer_fence.rs                       # the daemon's fence adapter (C003): authoritative
                                              # scope resolution, containment, the permit-backed
                                              # shared-checkout profile
```

`codegg-core` holds the domain types and the store. It has no dependency on the
TUI, the server, the tools, or the scheduler. The daemon adapter above it
supplies authorization, does the waiting, and owns the holder's release
contract. The split is enforced by `scripts/check-core-boundary.sh`.

The fence contract is the one place where the tool layer and the daemon meet,
and it meets them without a dependency: `src/tool/` never imports `crate::core`.
The tool layer holds a `dyn CheckoutWriterFence`; the daemon supplies the
implementation.

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

## The checkout writer fence (C003)

C002 built a permit that nothing consulted. C003 puts the fence in the writer
path, at the **final native effect** — the `fs::write`, the `fs::rename`, the
Git index and ref mutation — so the permit is taken *after* approval and
*before* the first effect, held across the write and any rollback, and
released only with evidence.

### Two rules, two different jobs

**Containment.** Every effect target must resolve inside the run's own
checkout. The comparison is on canonical paths, so a symlink pointing out of
the checkout buys nothing, and a *new* file cannot escape by naming a
directory that does not exist (the path is resolved through its nearest
existing ancestor). The roots the daemon granted alongside the checkout — the
run's private scratch root and its shared scratch roots — are part of the
scope: a scratch write is a legitimate write that is not a checkout write, and
the model never names those roots.

This is what closes the hole the `git` tool's model-supplied `workdir` used to
open. Before C003 a model could point the tool at any directory on the
machine and stage, commit or reset a repository the run did not own.

**Mutual exclusion.** A checkout shared by more than one run admits one writer
at a time. The share is not asserted by a caller: it is derived from the
durable group record. Two runs share a checkout when they are members of the
same *live* group and the lease belongs to one of them. The group is the
bounded reservation; the lease still fences the generation, and the permit
still records the individual run — a group is not a bearer token.

### Where the fence is consulted

| Surface | Class | Fenced at C003? |
|---|---|---|
| `write`, `edit`, `replace`, `apply_patch` | `FileWrite` | yes — before the first filesystem mutation |
| `git` typed mutations | `GitMutation` | yes — before the first index/worktree/ref effect |
| `git` raw argv fallback | `Unknown` | routed through the fence so it fails closed on a shared checkout |
| `git` read-only actions | — | no: a read is not a write, and readers overlap freely |
| process writers | `ProcessSpawn` | refused on a shared checkout; C005 owns the contract |
| non-broker sinks | — | C004 |

The broker *carries* the fence but never acquires from it. A tool that never
reaches an effect — a read, a denied call, a job submission — holds no permit
and writes nothing. That is also what keeps a nested dispatch from acquiring
the same checkout twice.

### Two profiles

An **isolated** checkout — the default placement, one run, its own managed
worktree — takes no permit. There is no second writer to exclude, so the
default placement pays no durable write and behaves exactly as it did before
C003, apart from the containment check.

A **shared** checkout takes the permit. On that profile an effect class with
no qualified profile is refused: there is no raw fallback and no best effort.
A shared write with no bound actor is refused too — mutual exclusion without
an attributed writer is an anonymous writer.

### Where it is not yet

The **containment** rule is live today. Every agent loop — the daemon turn,
the delegated-run worker, `codegg exec` and the CLI — builds a
`WorkspaceWriterFence` over the run's own workspace and hands it to every
tool execution context, so a native writer whose target resolves outside that
scope is refused before the effect.

The **permit-backed shared profile** is not live yet: `DaemonWriterFence` has
no production caller, because nothing in production places two runs on one
checkout. That is the milestone's boundary rather than an oversight — C006 is
what creates the shared placement, and wiring the profile in earlier would
enable shared mutation before C004 and C005 have qualified it.

So the shared profile is a **capability**, not a product path. Nothing in
production places two runs on one checkout: the work-order `shared`/`serialized`
placement is still refused, and the scheduler still gives every delegated run
its own isolated worktree. C004 (non-broker sinks) and C005 (process denial)
must complete the profile, and C006 must produce the real production path and
the native Q1–Q5 evidence, before any shared mutation exists.

## What This Does Not Claim

- **Not a filesystem exclusion.** The permit coordinates CodeGG's own writers.
  An unrelated process running as the same user is outside its guarantee.
- **Not a sandbox.** Nothing here contains a process.
- **Not an enablement.** C003 routes the native file writers and the typed Git
  mutations through the fence, and it closes the model-supplied `workdir`
  hole. It does **not** enable shared mutation: no production path places two
  runs on one checkout, and the shared profile refuses every effect class
  C004 and C005 have not yet qualified. C004, C005 and C006 own the rest.
- **Not a claim of mediation.** A permit existing does not mean any particular
  writer went through it. C003's own evidence is in
  `tests/writer_fence_c003.rs`, and it constructs the shared case directly
  rather than waiting for a product path to exist.

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

`tests/writer_fence_c003.rs` (9 tests) covers the fence: a write outside the
run's checkout refused with no filesystem effect, a write inside it allowed, a
daemon-granted scratch root inside the scope, the real `write` tool refusing
an out-of-scope target without creating the file, two runs on one checkout
never overlapping their mutating effects (the second writer is refused while
the first holds the checkout and takes it only after the first settles), a run
outside the live group refused, an unclassified effect and a process effect
refused on a shared checkout, a shared write with no bound actor refused, an
out-of-scope write refused before the permit is taken (so a refused write
never holds the checkout), and an isolated checkout taking no permit at all.

The `BEGIN IMMEDIATE` primitive the store depends on is shared with the
user-prompt queue store and is guarded by
`scripts/check_queue_write_transaction_shape.py`, whose self-test proves it
rejects both a deferred read-then-write path and a closing rollback.
