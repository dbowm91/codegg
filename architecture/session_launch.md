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
   `accepted` **resumes**: the call picks up from the stage the record
   reached. A request that is already `completed` converges and returns
   the recorded outcome with `converged = true`.
2. **The checkout.** For `NewManagedCheckout`, reserve the identity and
   locator, then create the worktree, advancing the saga between the
   two. For `ExistingManagedWorktree`, the named checkout must be
   *managed*, must belong to the launch's project and repository, and
   must not be **held** by another run.
3. **The workspace.** The session runs in the *checkout's* workspace, not
   the project's. It is registered at the checkout's path and bound to
   the launch's project, and the checkout id is recorded on the binding.
4. **The session.** Created, or forked from a parent. A fork inherits
   the parent's workspace and project, so both are moved into the
   checkout before it is bound — the session row and the canonical
   binding must agree.
5. **The binding.** `session_project_binding` records the session's
   project and workspace.

### A checkout is a second working directory, not a new project

The catalog's auto-provisioning rule gives every previously unseen
workspace root **its own logical project**
(`ProjectStorage::reconcile_workspace_path` → `resolve_project_and_repository`).
That rule is right for a directory the user opens and wrong for a launch.

A checkout is not a new project: it is a second working directory of the
project the caller already named. A launch that auto-provisioned here
would bind the session to a project nobody asked for, and the session
would not appear under the project the user launched from. Worse, the
binding would then *contradict* the session: `SessionStore::create_with_binding`
requires the workspace's binding to name the same project as the session,
so the launch would fail at the last step, after the checkout was already
on disk.

So the placement is explicit. `ProjectStorage::bind_checkout_workspace`
binds the checkout's workspace to the launch's project, shares the
launch's repository, and records the managed checkout it belongs to. It
is **refused** when the workspace is already bound to a different
project: a checkout that belongs to another project is not adopted by a
launch; the caller must name that project.

### The `worktree_id` that was never written

`workspace_project_binding.worktree_id` existed but was never written:
every binder stored `NULL`, and the upsert omitted the column from its
`DO UPDATE SET` list. A workspace that is a managed checkout therefore
could not be *named*.

M006 writes it. `bind_checkout_workspace` writes the whole binding —
project, repository, checkout id, locator, status and revision — in one
transaction, because the write is a read-then-write (the existing binding
is read before the row is written) and therefore opens with
`BEGIN IMMEDIATE` rather than a deferred transaction.

### The lease owner

The worktree lease is owned by an `AgentRunId`. A session the user
launched has no agent run behind it, so the launch derives a synthetic
identity from the launch id, and a retry reserves under the same owner.

The launch does **not** take the lease with it. The lease is what fences
writers, and the writer that works in this checkout is the session's own
agent run: a lease held by a synthetic owner would fence that run out
(`acquire_lease` refuses a checkout that already has an owner) and
nothing would ever release it, because a synthetic owner has no
lifetime. The checkout therefore stays `ready` and unleased, and the run
that works in it takes the lease when it needs one.

### A checkout another run holds is refused

A managed checkout whose `owner_run_id` is set is a **shared** working
tree: a run is working in it right now. M006 places a session in an
*isolated* checkout, so a launch that names a held checkout is refused
rather than quietly making a second session in the same directory. A
checkout whose previous run has *released* the lease is available again.

## Failure leaves a visible record

A launch that fails does not clean up. It advances its own record to
`needs_attention` with a bounded **category** — `checkout_failed`,
`workspace_failed`, `session_failed`, `invalid_request`, and so on — and
returns the error. The category is a category, never the detail: the
detail is the error's own message, which the operator surface shows in
full. A code carrying the detail would end up in a durable record and in
an operator's list, where it would be both unbounded and misleading.

Nothing is deleted. A dirty checkout and a half-made session are the
evidence an operator needs.

## Resuming

A resumed launch reads the stage the record reached and continues:

- the checkout is created only if the **worktree record** says it is
  still `reserved` — the worktree record, not the launch record, is the
  authority on whether the directory exists, because the two can
  disagree when a launch died between the two writes;
- the session is loaded, not created, when the record names one;
- a stage the launch already passed is skipped rather than re-recorded,
  because the store's transitions are forward-only and a repeat is not a
  contract violation.

The fork's child id is **derived** from the launch id
(`<launch id>-fork`), so a resume — or a retry — converges on the child
the first attempt made instead of forking a second one. The id is
derived rather than recorded so that the durable record never claims a
session exists before it does.

### The narrow windows

Two windows remain, and they are narrow because each is a single write
between two recorded facts:

- a launch that dies between the worktree **reservation** and the record
  that names it leaves a reservation with no directory and no record. The
  worktree service's own reconcile surfaces it; the launch is not
  corrupted.
- a launch that dies between the **session creation** and the record
  that names it leaves a session with no launch record. A retry derives
  the same child id and converges on it.

Both leave a *visible* artefact — a reserved checkout, a session in the
list — rather than a silent one.

## Only managed checkouts are addressable

An external checkout has no durable identity in this daemon: there is
nothing to bind a session to, and nothing to authorize later. Admitting
one would produce a session whose checkout the daemon cannot name. The
placement type therefore offers managed checkouts only, and the handler
refuses an external one.

A managed checkout that is not **ready** is refused too. `reserved` means
the identity was taken but the directory was never created; `orphaned`
means something went wrong. Both are for an operator, not for a session.

## The checkout may not be creatable on this host

`git worktree` is classified as requiring **contained** execution
(`egggit::process::classify_git_request` → `CleanSmudgeFilter`: a
checkout can run a smudge filter, which is arbitrary code). This
revision has **no contained Git backend**:
`GitEnvPolicy::apply` and `apply_sync` both turn a contained request into
`GitExecutionPolicyOutcome::BackendUnavailable`, and
`WorktreeService::create_reserved` — the only path that creates a
checkout — goes through the sync boundary.

So on this host the product's own checkout creation **always fails
closed**, and that is the plan's own anticipated condition:

> If the available Git backend cannot safely create a managed checkout on
> a platform, the request must fail closed and retain recoverable state.

What that means in practice:

- the launch refuses with the backend's own error, so the user learns
  *why* rather than seeing a generic failure;
- **no session is created** — the failure happens before the session
  step;
- the launch's durable record moves to `needs_attention` with the
  `checkout_failed` category, so the failure is visible to an operator
  rather than silent. The record is *terminal*: it is waiting for an
  operator, not for a resume, so it is found through the project's launch
  list rather than through the unfinished set;
- the **checkout record is left behind**, marked `orphaned` with a
  `git_error` health. That is the state the TUI's `⚠` marker reads, and
  the state the worktree service's own reconcile surfaces. The directory
  was never created — the `git worktree add` never ran — and the record
  is **not** cleaned up, because it is the evidence of what happened.

Nothing is cleaned up. A reserved-but-never-created checkout and a
half-made session are the evidence an operator needs.

`tests/session_launch_service.rs` proves both halves: the fail-closed
path, and the placement semantics against a **real** linked worktree
that the fixture creates with a plain `git worktree add` and records in
the state `create_reserved` would have set. The product's checkout
*creation* on a host with the contained backend is **not** claimed here.

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

## The TUI surface

The session dialog is the surface. Each row shows the checkout the session
runs in — the managed checkout's branch, or `root` for the project's own
working tree — a `⚠` when that checkout needs an operator, and the fork's
ancestry by name. Three keys place a session in a checkout: `n` for a new
session in a new checkout, `w` for a new session in the selected session's
*existing* checkout, and `F` for a fork into a new checkout. The plain `f`
fork is untouched.

`architecture/tui.md` §"The session checkout surface (M006, agent-workstream-blackboard)"
has the details.

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

`resolve_authorization_project` resolves the project for a launch the
same way it does for a session create. Without that arm the
`DirectProject` scope resolved to `None`, and a team principal's launch
was denied for a *missing scope* instead of for a missing grant — a
denial that named the wrong reason and asked for the wrong fix.

### The checkout half is a second capability

A launch has two effects: it creates a **session** and it may allocate a
**checkout**. The descriptor carries one capability, and it is
`session.create`.

The checkout is created *in the project the launch names* — the same
project the caller's `session.create` was authorized against — so the
project boundary holds. The capability boundary did not: a principal
that holds `session.create` and **not** `worktree.create` could still
cause a checkout to be allocated.

`operation_descriptor` carries exactly one capability by design, and the
executable matrix is derived from it, so the second capability cannot be
expressed there. M006 therefore asks for it separately.

`Daemon::authorize_launch_checkout` runs **before** the request is
dispatched, and before the allow event is recorded:

- a launch that allocates a checkout (`NewManagedCheckout`) is authorized
  for `Capability::WorktreeCreate` against the same project;
- a launch that names an already-managed checkout allocates nothing, so
  it does not ask for `worktree.create` — the checkout it names is
  already owned by the project, and the service refuses a checkout
  belonging to another project or another repository.

Because the check runs before dispatch, a denied launch has **no side
effect at all** — not even an accepted launch record — and the audit
trail holds one decision, not an allow followed by a denial.

Today no role separates the two capabilities: `Viewer` holds neither,
and `Contributor`, `Maintainer` and `Owner` hold both. The check is
therefore a guard on a *custom* grant, not on a role: a principal granted
`session.create` alone, without `worktree.create`, fails closed for a
launch that allocates a checkout.

## What this module does not do

- It does not enable a shared checkout or a shared mutation.
- It does not delete a failed launch's artefacts. A launch that fails
  leaves its evidence and a durable `needs_attention` record; silently
  deleting a checkout or a session would destroy what an operator needs.
- It does not make the checkout's branch the session's branch policy. The
  target branch is what the checkout is created on; nothing here decides
  what a later merge should do.
