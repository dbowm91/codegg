# ADR-0048: Contained Git Execution for Managed Worktree Creation

Status: accepted — 2026-10-22; authorizes exactly one contained Git operation shape

Date: 2026-10-22

Decision owners: project maintainers

Source architecture:
- plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md (accepted)
- plans/adrs/ADR-0032-agent-workstream-coordination-and-shared-checkout-ownership.md (accepted direction; M005 gate remains)
- plans/adrs/ADR-0046-composable-linux-execution-isolation.md (accepted)
- plans/adrs/ADR-0047-restricted-agent-shared-checkout-writer-contract.md (accepted)
- plans/000-long-term-specification.md §§4, 17, 27, 29
- plans/001-terminology-and-domain-model.md §8
- plans/003-planning-process.md

Affected subsystem roadmaps:
- plans/subsystems/linux-trusted-git-cost-corrective-roadmap.md
- plans/subsystems/linux-execution-isolation-roadmap.md
- plans/subsystems/agent-workstream-blackboard-m005-writer-containment-corrective-addendum.md

Related, **not** granted here:
- plans/adrs/ADR-0044-contained-git-live-objects-and-metadata-authority.md — **remains
  proposed and is not superseded by this ADR.** It continues to own the mutable
  object-read class and the trusted-metadata-promotion class. Neither is granted
  here.

## Context

CodeGG's isolated-checkout default is a Git *linked worktree*. The daemon creates
one through `WorktreeService::create`, which reaches `git worktree add`.

`egggit::process::classify_git_request` classifies `worktree` (other than
`worktree list`) as requiring the `CleanSmudgeFilter` capability, because
creating a worktree writes the branch's files through the repository's smudge
filter, and a smudge filter is arbitrary code. `GitEnvPolicy::{apply,
apply_sync}` turns any containment-requiring request into
`GitExecutionPolicyOutcome::BackendUnavailable`.

There is **no contained Git backend in this tree**. Consequently:

* `WorktreeService::create_reserved` always fails, so managed worktree creation
  always fails closed. `architecture/session_launch.md` records this as the
  tree's anticipated condition.
* A delegated mutation-capable child in a Git repository cannot obtain its
  isolated worktree, and the run fails with `isolation_unavailable`.
* The M005 writer-containment corrective's composed two-agent race cannot be
  composed, because the shared checkout is allocated through the same path.
  `plans/closure/agent-workstream-m005-writer-containment-corrective/006-status.md`
  §9.2 records this as a named non-claim.

The capability is **not** satisfiable by neutralizing repository configuration.
A smudge filter's *command* lives in configuration under a name the caller
cannot enumerate (`filter.<name>.clean` / `.smudge` / `.process`), so a
`-c` override cannot disable an unknown filter. Containment, not policy, is what
keeps repository-controlled code from reaching the host.

The mechanism already exists in one place: the production private-index
publication's contained staging step
(`src/git_publication/staging.rs::ContainedGitIndexStaging`) runs the
daemon-pinned Git executable through the managed-process owner with the M004
process/IPC tier and the M005 offline-network tier, asserts the *obtained*
enforcement, and sanitizes the environment. That step stages into a private
gitdir and never writes the repository's own metadata. This ADR generalizes the
mechanism to exactly one additional operation.

Two facts constrain the design, both verified against Git 2.43.0:

1. A linked worktree's administration directory is **always**
   `<common-gitdir>/worktrees/<name>`. `git worktree add` has **no**
   `--separate-git-dir` option — that option belongs to `git clone`. So the
   contained child must be able to write inside the source repository's own
   Git metadata.
2. ~~`git worktree add <path> <existing-branch>` writes **no ref**: it creates
   only `<common-gitdir>/worktrees/<name>/` and the working-tree files, and
   leaves `refs/heads/<branch>` untouched. `git worktree add <path> <new-branch>`
   *would* create the branch — and therefore *would* write a ref.~~
   **Corrected 2026-10-22 — see "Amendment: the attached form writes the
   branch ref after all" below.** The branch ref's *value* is unchanged, but
   Git **locks and rewrites** it when attaching, so the attached form cannot
   run inside a grant that excludes `refs`. The detached form writes no ref.

Fact 2 is what makes a narrow grant possible: if the daemon owns the branch, the
child has nothing to write outside the administration directory.

## Decision drivers

- A linked worktree is CodeGG's isolation model (ADR-0002). The alternative — a
  shared-alternates clone — would change `WorktreeService` semantics, parent
  integration and worktree listing, and is not a containment question.
- The contained child runs repository-controlled code. The host must be
  protected by an **enforced** boundary, and the boundary that is enforced must
  be the one that is asserted.
- The grant must be as narrow as the operation allows. A child that can write
  refs can repoint `main`.
- A missing capability must be a typed refusal, never a silent downgrade to an
  uncontained path.
- The grant must be auditable against production code, not against a test.

## Considered options

### A — Accept ADR-0044 as written, and use its contained-execution class

ADR-0044 is proposed and covers three distinct things: contained Git
execution, mutable (live) object reads, and trusted metadata promotion. Only
the first is needed here. The mutable-read class sits behind a closed
no-admission decision and a closed-negative finding — Linux C010 no-admission
(`plans/closure/linux-execution-isolation/018-status.md`) and C012 closed
negative (`plans/closure/linux-execution-isolation/020-status.md`), whose
Btrfs fragmented cold p95 of 1,109.391 ms missed C010's 1,000 ms gate and
whose "a read-only bind mount or in-process mutex is not an immutable
snapshot" finding remains valid. ADR-0044's own acceptance gate also requires
an independent security review, native supported-Linux and restricted-userns
negative proofs, real-tool equivalence and performance evidence, exact head CI,
rollback, and an explicit operator release decision before **any** live-ODB
trusted read or private promotion route is enabled. Accepting it would grant
authority for two classes this milestone does not need and whose evidence is
incomplete. **Rejected.**

### B — A narrow ADR for contained worktree creation only

Grant exactly the authority the operation requires, under its own acceptance
record, leaving ADR-0044 proposed and untouched. ADRs are immutable once
accepted; a narrower decision is a new document, not a rewrite of a broader one.
**Selected.**

### C — Abandon linked worktrees for shared-alternates clones

A shared clone never writes to the source repository's Git metadata at all, so
no carve-out is needed. But it is not a linked worktree: it changes
`WorktreeService`'s identity, lease and listing model, changes parent
integration, and departs from ADR-0002's worktree model. It trades a narrow
containment grant for a broad product-model change. **Rejected for this
milestone.**

### D — Neutralize repository configuration instead of containing the child

Disabling every executable configuration surface by name. Rejected: filter
names cannot be enumerated, so unknown filters cannot be disabled, and the
result would be a false security claim.

## Decision

1. **One operation shape.** This ADR authorizes exactly one contained Git
   operation: `git worktree add <path> <existing-branch>`, executed inside an
   OS-contained child. It authorizes no other Git subcommand, no other
   capability class, and no change to any default policy. Every other
   containment-requiring Git request remains `BackendUnavailable`.
   **Amended 2026-10-22 — see "Amendment: the attached form writes the branch
   ref after all" below.** The operation is unchanged in kind and is now
   carried out in two steps: the worktree is created **detached**, and the
   daemon's branch is attached to it with `git symbolic-ref`. Both steps run
   inside the same grant. The child still writes no ref.

2. **The branch belongs to the daemon.** The daemon creates the branch through
   the existing typed, policy-allowed `git branch` path **before** the
   contained child runs, and the child is given the name of a branch that
   already exists. The child may not create, move, delete or repoint any ref.
   This is enforced, not merely intended: the child's writable set excludes
   `<common-gitdir>/refs`, so a child that attempted to create a branch fails
   closed instead of creating one.

3. **The writable set.** The contained child may write exactly three things:
   the new worktree directory, the daemon-owned scratch root, and
   `<common-gitdir>/worktrees/` — the **shared** administration root. That
   root is the **parent** of every linked worktree's own administration
   directory, so the child may write another linked worktree's `HEAD` under
   it; the amendment below records that consequence as an open question.
   Every other path under the Git metadata, including `refs`, `objects`,
   `index`, `config`, `hooks` and `packed-refs`, is read-only to the child.
   The narrower grant — the linked worktree's own administration directory
   alone — is **not** granted; narrowing the grant to it is a new ADR.

4. **Containment is asserted, not requested.** The child is a daemon-pinned Git
   executable launched through the managed-process owner with the M004
   process/IPC tier and the M005 offline-network tier. The launch's *obtained*
   enforcement is checked after the launch; a launch that did not obtain
   enforcement is a failure, not a degraded success.

5. **The sandbox is the security property, not the configuration.** The child
   runs with a sanitized environment, `GIT_CONFIG_NOSYSTEM=1`,
   `GIT_CONFIG_GLOBAL=/dev/null`, `GIT_ATTR_NOSYSTEM=1`,
   `GIT_TERMINAL_PROMPT=0`. The repository's own configuration may still
   define filters, hooks, merge drivers, fsmonitor, diff drivers and credential
   helpers. The child therefore remains **untrusted code**, and it is the
   sandbox — not the absence of that configuration — that keeps it from the
   host. No claim of "no filter could run" is made; the claim is "a filter that
   runs cannot reach the host".

6. **Fail closed, never downgrade.** If the platform cannot express the narrow
   writable grant, or the launch cannot obtain enforcement, worktree creation
   is refused with a typed reason that names the missing capability. There is no
   uncontained fallback, no "create the worktree without the sandbox" path, and
   no partial success reported as success.

7. **Registration is the acceptance criterion.** A created checkout is accepted
   only when `git worktree list` reports it at the recorded path **on the
   recorded branch**, and the checkout is a real linked worktree. A detached
   worktree is not an acceptable result: `git worktree list` reports a detached
   checkout as `(detached HEAD)`, which does not match the recorded branch and
   would leave the record inconsistent with the repository.

8. **The daemon owns the launch.** The authority is `codegg-core`'s worktree
   service through a named contained executor. The daemon builds it; a test may
   substitute its own implementation, exactly as the private-index publication
   permits, but no test-only path may stand in for the production route in a
   closure claim.

9. **Not granted.** This ADR grants no mutable or live object-read class, no
   trusted metadata promotion, no remote transport, no credential mediation,
   and no change to the immutable trusted-read route or to `GitEnvPolicy`'s
   classification of any other operation. ADR-0044 remains proposed.

## Acceptance record

Decision: **option B accepted**, 2026-10-22, by the project maintainers
(David Bowman), on branch `work/linux-trusted-git-c008-contained-worktree`.

Provenance of the decision package:

- The decision was requested while scoping the M005 writer-containment
  corrective's remaining non-claim — the composed two-agent daemon race —
  recorded at `plans/closure/agent-workstream-m005-writer-containment-corrective/006-status.md`
  §9.2.
- The maintainer was asked to choose, and chose, that the race be composed with
  a **scripted** provider rather than a live LLM provider, that the contained
  Git authority be a **narrow new ADR** rather than acceptance of ADR-0044,
  and that the writable grant be **`<common-gitdir>/worktrees/` only**, with the
  branch owned by the daemon.

What this acceptance does:

1. It selects option B as the authority for contained worktree creation and
   makes clauses 1–9 above the governing contract for the implementation
   milestone `plans/implementation/linux-trusted-git-cost-corrective/008-contained-worktree-creation.md`.
2. It grants **only** the operation shape in clause 1. C005's broader parity,
   performance and admission scope is untouched and remains blocked.
3. It does **not** accept ADR-0044 and does not supersede it. The mutable
   object-read and metadata-promotion classes remain ungranted.
4. It records that the platform matrix for the shared-checkout profile stays
   **Linux and macOS**. Windows has no filesystem-enforcement backend
   registered at all, so it stays disabled with a named reason rather than
   becoming an open task.

## Consequences and tradeoffs

Positive:

* Managed worktree creation stops failing closed, so the isolated-checkout
  default works again for delegated children in a Git repository.
* The M005 composed two-agent race becomes composable, which is the last named
  non-claim on that milestone.
* The grant is narrow enough to state in one sentence, and the enforcement
  mechanism already exists.

Negative:

* The contained child is still untrusted code running on the host's kernel
  boundary. The sandbox reduces the blast radius; it does not make the
  repository's filters safe to run.
* Worktree creation costs two launches and a sandbox setup, so it is slower
  than an uncontained `git worktree add`. It is not on a hot path.
* The daemon's own branch write happens outside the sandbox, under the
  existing typed-Git authority. That is existing product behavior and is out of
  this ADR's scope, but it means the *branch creation* is not contained.
* The child can still write the working tree, so a hostile filter can corrupt
  the files it checks out. The checkout is untrusted content until the
  repository's own trust model says otherwise.

Neutral:

* `WorktreeService`'s record shape does not change. The branch field already
  holds the branch name, and the registration check already exists.
* Nothing about the shared-checkout profile's platform gate changes.

## Compatibility and migration

* No schema migration. The worktree record, its states and its lease model are
  unchanged.
* No wire or protocol change. The contained executor is an internal seam
  between `codegg-core`'s worktree service and the managed-process owner.
* Existing behaviour on hosts that cannot obtain enforcement is unchanged:
  worktree creation is refused with a typed reason and the reservation is left
  orphaned with `git_error` health, exactly as today.
* The `git` tool's own branch operations are unchanged. The daemon's branch
  creation uses the same typed path it already uses.

## Security, reliability and stop conditions

* The contained child's writable grant is the security boundary. If the
  platform's sandbox cannot express "writable administration directory inside a
  read-only Git metadata tree", the milestone **stops**; it does not widen the
  grant to the whole Git metadata, and it does not fall back to an uncontained
  child.
* The obtained enforcement is read from the launch's own result. A launch that
  reports a request but not an obtained fact is a failure.
* Hostile-configuration fixtures must demonstrate that a smudge filter, a
  hook, a merge driver, an fsmonitor hook and a credential helper cannot
  reach the host, and that the child's writes stay inside its grant.
* A containment regression that lets the child write outside its grant, or a
  worktree that is created without obtained enforcement, is a **stop** — the
  milestone records a negative result rather than a weakened guarantee.

## Verification

* The grant is asserted structurally: the child's writable set contains the
  administration directory and the new checkout, and not the repository's refs,
  objects, index or configuration.
* Positive: a managed worktree is created through the production path, is
  reported by `git worktree list` on the recorded branch, and is a real linked
  worktree whose files are Git-verified.
* Negative: a repository carrying a hostile smudge filter, hook, merge driver,
  fsmonitor hook and credential helper produces no host-side effect outside the
  grant, and the checkout is still created.
* Negative: a host that cannot obtain the M004/M005 tiers refuses with a typed
  reason and leaves no half-made checkout that claims success.
* Native Linux evidence is required on a qualified host (Landlock with the
  M004 process/IPC tier and the M005 offline network). macOS reuses the
  Seatbelt evidence already qualified for the shared-checkout profile.

## Supersession

* This ADR does **not** supersede ADR-0044, which remains proposed and owns the
  mutable object-read and metadata-promotion classes.
* This ADR does **not** supersede ADR-0002, ADR-0032, ADR-0046 or ADR-0047.
* A future decision that widens the contained Git operation set — a second
  subcommand, a second capability class, or a mutable read — is a new ADR, not
  an edit to this one.

## Amendment: the attached form writes the branch ref after all

Amended 2026-10-22, after the contained child was run against a real
repository. This amendment corrects **Fact 2** and changes **clause 1**. It
does not widen the grant, weaken a clause, or change the decision's option.

### What was measured

Fact 2 as written above says that `git worktree add <path> <existing-branch>`
writes no ref. The measurement behind it checked the branch ref's **value**,
and the value is indeed unchanged. That was the wrong thing to measure.

With `refs/heads` made unwritable — which is what ADR-0048's grant produces —
the attached form fails:

```
error: update_ref failed for ref 'HEAD': cannot lock ref 'HEAD':
Unable to create '.../refs/heads/<branch>.lock': Read-only file system
```

Git locks and rewrites the branch ref when it attaches a worktree to it, even
though the value it writes is the value the branch already had. Measured on Git
2.43.0 and 2.55.0. The detached form, `git worktree add --detach <path>
<commit>`, writes **no ref at all** and succeeds against the same read-only
`refs/heads`.

So Fact 2 is false as written, and the conclusion drawn from it — "if the
daemon owns the branch, the child has nothing to write outside the
administration directory" — does not hold for the attached form. The attached
form needs the child to write inside `refs/heads/`, which the grant — and
ADR-0048's decision drivers, which say "a child that can write refs can
repoint `main`" — both refuse.

### What changes

**Clause 1** now grants one operation in **two steps**:

1. `git --git-dir=<gitdir> worktree add --detach <checkout> <commit>` — the
   child creates the linked worktree detached. It writes the administration
   directory and the checkout's files, and no ref.
2. `git --git-dir=<administration-dir> symbolic-ref HEAD refs/heads/<branch>` —
   the child attaches the daemon's branch to the new worktree, writing only the
   administration directory's own `HEAD`, which the grant already names.

Both steps run inside the same grant and the same containment. The second step
is not a second operation class: it is the same `symbolic-ref` plumbing Git
itself uses to record a worktree's branch, and it writes nothing outside the
administration directory the first step created.

**Clause 2** is unchanged in force and stronger in fact: the child writes **no
ref at all** — not the branch, not any other. The branch is created by the
daemon before the child runs, through the typed `git branch` path that already
exists. The child only points the new worktree's `HEAD` at it.

**Clause 7** is unchanged: a created checkout is accepted only when `git
worktree list` reports it at the recorded path **on the recorded branch**. The
attachment is what makes that true, and the check reads the result back through
Git itself. A detached worktree is still not an acceptable result — it is an
intermediate state that the second step must complete.

### What does not change

* The writable set is still exactly `{checkout, scratch, <common-gitdir>/worktrees/}`.
* The Git directory is still read-only to the child, `refs` included.
* The sandbox is still the security property, and configuration neutralization
  is still not one.
* The launch still fails closed and never downgrades.
* No change to `GitEnvPolicy`'s classification of any operation — `symbolic-ref`
  is not run on the ordinary process boundary; it runs inside the containment,
  where the daemon's argv is the authority.
* No mutable object read, no metadata promotion, no remote transport, no
  credential mediation. ADR-0044 remains proposed.

### Why the attach step runs inside the containment, and not on the host

The daemon could attach the branch itself, on the host, after the child exits.
It does not, for one reason: the administration directory is inside the child's
grant, so a hostile filter could write that directory's `HEAD` and point the
worktree at a branch of its choosing. Running the attach as the child's **last**
step, with a target fixed by the daemon's argv, means the worktree always ends
on the daemon's branch — the hostile write is overwritten rather than trusted.
The registration check in clause 7 then reads the end state back through Git.

The cost is one additional contained launch on a path that is not hot: the
branch creation, the worktree creation and the attach are three launches, not
two.

### Why the alternative was not taken

Granting the child write access to its own branch — by binding
`<common-gitdir>/refs/heads/codegg/worktree/` writable, for instance — would
have kept a single launch. It was rejected: the child would then be able to
repoint **any** managed worktree's branch, which is exactly the "a child that
can write refs can repoint `main`" failure this ADR exists to prevent, and it
would contradict clause 2 outright.

## Amendment: the shared administration root, and the capability it carries

Amended 2026-10-23, after a clause-by-clause review of this ADR against the
implementation it governs. This amendment corrects **clause 3**'s
*description* and records a **consequence** the clause did not carry. It does
not change the accepted path, weaken a clause, or change the decision's
option.

### What was wrong

Clause 3 said the child may write `<common-gitdir>/worktrees/` — "the linked
worktree's own administration directory". The **path** is right; the
**description** is not. `<common-gitdir>/worktrees/` is the **shared**
administration root: the **parent** of every linked worktree's own
administration directory, not one of them.

The implementation grants the shared root, which is what the acceptance record
chose — "the writable grant be `<common-gitdir>/worktrees/` only". So the
defect was in the prose, and in the artifacts that repeated it: this ADR's
clause 3, two comments in `src/git_contained_worktree.rs`, and the
containment test in `tests/contained_worktree_c008.rs` — the test whose job
is to assert the grant. Five artifacts described a narrower grant than the
code delivers.

The consequence of the shared root is recorded below because it is real, and
because the clause's own wording hid it.

### What changes

**Clause 3** now names `<common-gitdir>/worktrees/` as the **shared**
administration root, states plainly that it is the parent of every linked
worktree's own administration directory, and states that the narrower grant —
the linked worktree's own administration directory alone — is not granted.

The accepted **path** is unchanged: `<common-gitdir>/worktrees/`, and only
that.

The code's two grant comments and the containment test's name and comment
agree with it. A static guard, `scripts/check_contained_worktree_grant.py`,
asserts that they continue to agree: that the grant's writable roots under
the repository's Git directory are exactly one path, that the path is the one
this ADR names, and that clause 3 and the code's comments describe it as the
shared root rather than as a linked worktree's own directory.

### What does not change

* The accepted path: `<common-gitdir>/worktrees/` only, with the branch owned
  by the daemon.
* The writable set: still exactly `{checkout, scratch,
  <common-gitdir>/worktrees/}`.
* The Git directory is still read-only to the child, `refs` included, so the
  child still writes **no ref** and cannot create a branch. Clause 2 holds as
  written.
* The operation is still one operation in two steps, and the attach step
  still runs last with the daemon's fixed target.
* **No implementation behaviour changes.** The delivered grant was already
  the shared root; only the description of it was wrong.

### The consequence the shared root carries

The shared root is the **parent** of every linked worktree's administration
directory, so the child can write **any** of them — including another linked
worktree's `HEAD`. The path to that write is real: `git worktree add` runs
the repository's `post-checkout` hook, and the contained child's argv does not
pin `core.hooksPath`. A repository that declares such a hook therefore runs
it inside the containment, and the hook can repoint another worktree's branch
pointer.

This is not a new capability of the child. It is what the accepted path
always permitted, and the clause did not say so. It is recorded here because
this ADR's own decision drivers refuse exactly it — "a child that can write
refs can repoint `main`" — and the shared root is a way to reach that outcome
without writing `refs`.

What is **not** affected: the child's own `refs` remain read-only.
`refs/heads/main` is under `<common-gitdir>/refs`, not under
`<common-gitdir>/worktrees`. The child cannot create a branch, and clause 2
still holds as written.

### Disposition: an open capability question, not an acceptance

**The maintainer has not chosen a disposition for this consequence, so this
amendment does not record one.** The consequence is recorded here as an **open
capability question**, and the milestone that corrected the prose stops at
the documentation correction rather than recording an acceptance that was not
made.

The question is whether the shared root's reach over **other** linked
worktrees' `HEAD` files is an acceptable consequence of ADR-0048's accepted
path. The candidates are listed for the maintainer to choose among; this
amendment selects none of them:

* **Accept as recorded** — the shared root stays, and the reach over other
  worktrees' `HEAD` files becomes an accepted, documented consequence of this
  ADR.
* **Narrow the grant** to the linked worktree's own administration directory.
  This **changes the accepted contract** and requires a **new ADR**; it is not
  an edit to this one, and this milestone does not do it.
* **Defence in depth** — pin `core.hooksPath` for the contained child, so the
  repository's `post-checkout` hook does not run inside the containment. This
  removes the most obvious path to the capability; it does not close it,
  because the shared root stays writable.

Until the maintainer chooses, the milestone
`plans/implementation/linux-trusted-git-cost-corrective/009-contained-worktree-grant-precision.md`
records this as an open question and stops at the documentation correction.
