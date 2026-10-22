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
2. `git worktree add <path> <existing-branch>` writes **no ref**: it creates
   only `<common-gitdir>/worktrees/<name>/` and the working-tree files, and
   leaves `refs/heads/<branch>` untouched. `git worktree add <path> <new-branch>`
   *would* create the branch — and therefore *would* write a ref.

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

2. **The branch belongs to the daemon.** The daemon creates the branch through
   the existing typed, policy-allowed `git branch` path **before** the
   contained child runs, and the child is given the name of a branch that
   already exists. The child may not create, move, delete or repoint any ref.
   This is enforced, not merely intended: the child's writable set excludes
   `<common-gitdir>/refs`, so a child that attempted to create a branch fails
   closed instead of creating one.

3. **The writable set.** The contained child may write exactly three things:
   the new worktree directory, the daemon-owned scratch root, and
   `<common-gitdir>/worktrees/` — the linked worktree's own administration
   directory. Every other path under the Git metadata, including `refs`,
   `objects`, `index`, `config`, `hooks` and `packed-refs`, is read-only to the
   child. This is the narrowest grant a linked worktree admits, because Git
   always places the administration directory inside the common Git directory.

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
