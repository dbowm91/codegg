# Session Launch — placing a session in an isolated checkout

M006. This module owns one thing: **launching a session into an
isolated checkout**, recoverably and idempotently.

It is not the shared-checkout milestone. Nothing here enables a shared
checkout, a shared mutation, or a second writer.

## Why the request itself is durable

Launching a session into a checkout spans two resources that cannot
share one transaction:

- the **checkout** is a Git worktree on the filesystem, and
- the **session** is a row in SQLite.

There is no atomic multi-resource commit. So the milestone does not try
to pretend otherwise. Instead, the *request* is recorded durably
(`session_launch`) and advanced through explicit stages.

That record is what makes a retry converge. Without it:

- `WorktreeService::reserve` mints a fresh `WorktreeId` per call, so a
  retried launch would allocate a **second** checkout;
- `SessionStore::fork` mints a fresh session id per call, so a retried
  launch would create a **second** session.

With it, the retry finds its own prior progress.

## The stages

```
accepted ──▶ checkout_reserved ──▶ checkout_ready ──▶ session_created ──▶ bound ──▶ completed
                 │                      │                   │                │
                 └──────────────────────┴───────────────────┴────────────────┴──▶ needs_attention
```

A stage is advanced only when the work it names has actually happened,
and the row is written before the next step begins. A launch that dies
half-way leaves a row at a known stage, and
`SqliteSessionLaunchStore::reconcile` reports exactly which stage it
reached and what it still owes.

`needs_attention` is terminal until an operator resolves it. A launch
never silently re-enters a stage, and a backward transition is refused.

## Idempotency

A launch is keyed by `(project_id, idempotency_key)`.

- A retry with the same key and the same **request fingerprint**
  converges on the existing row.
- A retry with the same key and a *different* fingerprint is **refused**
  with `IdempotencyConflict`. It is a different request wearing an old
  key; running it would attach a new checkout to the old key's record.

The fingerprint covers everything that changes what the launch *does* —
the project, the intent, the placement, the base ref, the target branch
and the parent session. It deliberately excludes the idempotency key
itself: the key identifies the request, it is not part of it.

The accept path is a read-then-write, so it opens its transaction with
`BEGIN IMMEDIATE` (the shared `ImmediateWrite` primitive). A deferred
transaction takes no lock at `BEGIN`, and the first `UPDATE` would have
to upgrade a shared read lock — which SQLite refuses without waiting.

## What the daemon does with it

`SessionLaunchService` (`src/core/session_launch.rs`) drives the launch:

1. **Accept** the request durably. A request that is already past
   `accepted` converges: the call returns the recorded outcome with
   `converged = true`.
2. **The checkout.** For `NewManagedCheckout`, reserve the identity and
   locator, then create the worktree, advancing the saga between the
   two. For `ExistingManagedWorktree`, the named checkout must be
   *managed* and must belong to the launch's project.
3. **The workspace.** The session runs in the *checkout's* workspace, not
   the project's. It is registered at the checkout's path, bound to the
   project, and the checkout id is recorded on the binding.
4. **The session.** Created, or forked from a parent. A fork inherits
   the parent's workspace, so it is moved into the checkout before it is
   bound — the session row and the canonical binding must agree.
5. **The binding.** `session_project_binding` records the session's
   project and workspace.

### The `worktree_id` that was never written

`workspace_project_binding.worktree_id` existed but was never written:
every binder stored `NULL`, and the upsert omitted the column from its
`DO UPDATE SET` list. A workspace that is a managed checkout therefore
could not be *named*.

M006 writes it, through `ProjectStorage::record_workspace_worktree`.
The write is guarded: a workspace that is not already bound to a project
is left alone, so this can never attach a checkout to an unresolved
workspace.

### The lease owner

The worktree lease is owned by an `AgentRunId`. A session the user
launched has no agent run behind it, so the launch mints a synthetic
one, derived from the launch id. A retry therefore converges on the same
owner instead of orphaning the first lease.

## Only managed checkouts are addressable

An external checkout has no durable identity in this daemon: there is
nothing to bind a session to, and nothing to authorize later. Admitting
one would produce a session whose checkout the daemon cannot name. The
placement type therefore offers managed checkouts only, and the handler
refuses an external one.

## The base ref and the target branch

`CreateWorktreeRequest` gained two optional fields:

- `base_ref` — a **branch or tag name**, resolved to an immutable
  commit when the checkout is reserved. The record stores the commit,
  never the moving name.
- `target_branch` — the branch the new checkout is created on. When it
  is omitted, the worktree service mints `codegg/worktree/<id>` as
  before.

`eggit::refs::resolve_base_ref` is deliberately **separate** from
`eggit::refs::resolve_commit`. The latter refuses anything but a full
object id because its callers use the result as *provenance*. A worktree
base is not provenance: it names where a checkout starts. The resolver
is still narrow — `..`, `~`, `^`, `:`, `@{`, a leading `-` and any
whitespace are refused, so it cannot be talked into evaluating an
arbitrary Git expression.

## The request surface

`CoreRequest::SessionLaunch` is a **new** variant, not an overload of
`SessionFork`. A fork *copies history*; a launch *places* a session.
Overloading `SessionFork` with a placement would make "fork into the
parent's workspace" and "fork into a new checkout" indistinguishable on
the wire.

The response is `CoreResponse::SessionLaunch`, which carries the
**concrete** identities the launch produced — the session, the
workspace and the checkout — rather than an `Ack` that leaves the caller
to find the session by listing them.

## Authorization

The launch's operation descriptor is `session_launch`, scoped
`DirectProject`, carrying `Capability::SessionCreate`. The scope is the
project the launch names, so a principal that is not a member of that
project fails closed.

### The known gap: the checkout half

A launch has two effects: it creates a **session** and it may allocate a
**checkout**. The descriptor carries one capability, and it is
`session.create`.

The checkout is created *in the project the launch names* — the same
project the caller's `session.create` was authorized against — so the
project boundary holds. But the capability boundary does not: a
principal that holds `session.create` and **not** `worktree.create` can
still cause a checkout to be allocated.

`operation_descriptor` carries exactly one capability by design, and the
executable matrix is derived from it, so the second capability cannot be
expressed there. Closing this properly needs either a second
authorization call in the handler — which requires the request's
authority context to be plumbed into the family handler — or a change to
the descriptor shape. Both are recorded as outstanding in the M006 plan;
M006 is **not closed** while this stands.

## What this module does not do

- It does not enable a shared checkout or a shared mutation.
- It does not delete a failed launch's artefacts. A launch that fails
  leaves its evidence and a durable `needs_attention` record; silently
  deleting a checkout or a session would destroy what an operator needs.
- It does not make the checkout's branch the session's branch policy. The
  target branch is what the checkout is created on; nothing here decides
  what a later merge should do.
