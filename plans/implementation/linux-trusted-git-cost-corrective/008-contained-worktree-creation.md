# Linux Trusted Git Cost Corrective Milestone 008 — Contained Managed Worktree Creation

Status: active

Repository baseline: `origin/main` at `47f5572b1` (branch `work/linux-trusted-git-c008-contained-worktree`)

Source roadmap: `plans/subsystems/linux-trusted-git-cost-corrective-roadmap.md` §6

Long-term requirements:
- `plans/000-long-term-specification.md` §§4 (isolation default), 17 (Git execution authority), 27, 29
- `plans/001-terminology-and-domain-model.md` §8

Applicable ADRs:
- `plans/adrs/ADR-0048-contained-git-execution-for-managed-worktree-creation.md` (**accepted 2026-10-22 — this milestone's governing authority**)
- `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md` (accepted)
- `plans/adrs/ADR-0046-composable-linux-execution-isolation.md` (accepted)
- `plans/adrs/ADR-0044-contained-git-live-objects-and-metadata-authority.md` (**proposed — not granted here, not superseded**)

Primary class: capability (restores a production capability that currently fails closed)

## 1. Objective

Make `WorktreeService::create_reserved` create a real managed linked worktree
through the **production** path, by running `git worktree add` inside an
OS-contained child whose writable grant is the linked worktree's own
administration directory and nothing else.

One bounded outcome: a managed worktree exists, is reported by
`git worktree list` on its recorded branch, and was produced by a contained
child whose obtained enforcement was asserted.

## 2. Why this milestone is ready

- **ADR-0048 is accepted** (2026-10-22) and names exactly one authorized
  operation shape. The authority question that blocked this work is resolved.
- **The mechanism exists.** `src/git_publication/staging.rs`'s
  `ContainedGitIndexStaging` already runs the daemon-pinned Git executable
  through `ManagedProcessService` with the M004 process/IPC tier and the M005
  offline-network tier, asserts the *obtained* enforcement, and sanitizes the
  environment. This milestone reuses that shape rather than inventing a
  second one.
- **The platform evidence exists.** Landlock (Linux) and Seatbelt (macOS) are
  already qualified for the shared-checkout profile under C006 §5.1.
- **The blocker is exactly one missing piece.** `egggit::process::GitEnvPolicy`
  turns every containment-requiring Git request into `BackendUnavailable`
  because no contained backend exists. C007's private-index publication proved
  the *staging* case; the *worktree* case is unbuilt.

Interface dependencies are stable: `WorktreeService::reserve` /
`create_reserved`, `WorktreeRecord`, `ManagedWorktreeState`, the
`WorktreeStore` trait and `SandboxConfig::launch_spec` are all existing,
tested surfaces.

## 3. Current implementation evidence

- `crates/codegg-core/src/worktree_service.rs:934` — `create_reserved` calls
  `crate::worktree::create_worktree_at`, which reaches
  `egggit::process::GitEnvPolicy::apply_sync`.
- `crates/codegg-core/src/worktree.rs:51` — `create_worktree_at` renders
  `git worktree add`.
- `crates/egggit/src/process.rs:562` — `ensure_trusted_git_request` maps
  `RequiresContainedExecution` to `BackendUnavailable`.
- `crates/egggit/src/process.rs:534` — `worktree` (other than `worktree list`)
  requires `CleanSmudgeFilter`.
- `architecture/session_launch.md` §"The checkout may not be creatable on this
  host" — the tree's own statement that managed checkout creation always fails.
- `src/scheduler/executors.rs:996` — a delegated mutation-capable child in a
  Git repository fails with `isolation_unavailable`.
- `tests/shared_checkout_c006.rs:220-225` — the M005 carrier writes the
  worktree record straight into SQLite *because* managed worktree creation is
  unavailable in this tree.
- Verified on this host against Git 2.43.0: `git worktree add <path>
  <existing-branch>` writes only `<common-gitdir>/worktrees/<name>/` and the
  working-tree files, and leaves `refs/heads/<branch>` untouched.
- Verified on this host against Git 2.43.0: `git worktree add` has **no**
  `--separate-git-dir` option — a linked worktree's administration directory is
  always inside the common Git directory.

## 4. Invariants that must not regress

1. **`egggit` never mutates.** The contained child is a *process* boundary
   owned by the root crate; `egggit` keeps its read-only role.
2. **Single parser truth.** The contained argv is rendered by the existing
   typed Git renderer (`codegg-git`), never by a second parser.
3. **Containment is asserted, not requested.** A launch that reports a request
   but not an obtained fact is a failure.
4. **No uncontained fallback.** If the contained path fails, the milestone
   fails; it never retries uncontained.
5. **Every other Git operation keeps its current classification.** This
   milestone adds one operation; it does not relax `GitEnvPolicy` for any
   other request.
6. **The daemon owns the launch.** A test may substitute its own
   implementation of the seam; no test-only path may stand in for the
   production route in a closure claim.
7. **The worktree record's branch stays truthful.** `create_reserved` already
   verifies that `git worktree list` reports the recorded branch; a detached
   worktree does not satisfy it and must not be reported as success.
8. **`codegg-core` stays UI/server/plugin/auth-free**, and the root crate
   stays the only owner of process spawning.

## 5. Scope

**In**

- A contained worktree-creation seam: a trait in `codegg-core`, implemented in
  the root crate.
- The root-crate implementation: the daemon-pinned Git binary, the contained
  launch, the obtained-enforcement assertion, the environment sanitization,
  and the exact argv.
- The sandbox grant composition that makes
  `<common-gitdir>/worktrees/` writable and the rest of the Git metadata
  read-only.
- The daemon branch creation that precedes the contained child, using the
  existing typed `git branch` path.
- Typed failure reasons for every refusal.
- Focused tests, a hostile-configuration negative test, and the closure
  record.

**Out**

- Any other Git subcommand. `git add`, `git commit`, `git merge`,
  `git cherry-pick`, `git rebase`, `git stash`, `git am`, `git bisect` and
  remote transport keep their current classification.
- The mutable object-read class and the trusted-metadata-promotion class.
  **ADR-0044 is untouched and remains proposed.**
- C005's broader parity, performance-benchmark and admission scope. C005 stays
  blocked on its own named blockers.
- The composed two-agent race. That is a **separate milestone** in the
  blackboard line, dependent on this one; its plan is written against this
  milestone's shipped API.
- Windows. The shared-checkout profile's platform gate stays Linux and
  macOS. Windows has no filesystem-enforcement backend registered at all, so
  this milestone produces no Windows evidence and claims none.
- Any change to `WorktreeService`'s record shape, lease model or states.

## 6. Required production changes

### 6.1 `codegg-core` — the seam

- A trait (working name `ContainedWorktreeCreator`) with a request and an
  outcome, living beside `WorktreeService`.
- `WorktreeService` gains an optional contained creator. When present,
  `create_reserved` uses it. When absent, the existing path is unchanged —
  and it still fails closed at the Git policy, exactly as today.
- The trait must not reference process, sandbox or platform types:
  `codegg-core` stays free of them.

### 6.2 Root crate — the contained executor

- A module beside `src/git_publication/` (working name
  `src/git_contained_worktree.rs`), mirroring `ContainedGitIndexStaging`:
  daemon-pinned Git binary, `SandboxConfig` with `SandboxMode::WorkspaceWrite`
  and the M004 process/IPC + M005 offline-network tiers, the obtained
  enforcement asserted **after** the launch, and a sanitized environment with
  `GIT_CONFIG_NOSYSTEM=1`, `GIT_CONFIG_GLOBAL=/dev/null`,
  `GIT_ATTR_NOSYSTEM=1`, `GIT_TERMINAL_PROMPT=0`, `LC_ALL=C`.
- The argv is exactly one shape: `git worktree add <path> <existing-branch>`,
  rendered through the typed renderer, with the branch already existing.
- The grant: the new worktree directory, the daemon scratch, and
  `<common-gitdir>/worktrees/`. Everything else under the Git metadata is
  read-only to the child.
- The child may not write `refs`, `objects`, `index`, `config`, `hooks` or
  `packed-refs`. The implementation must **prove** this, not assert it in a
  comment.

### 6.3 Sandbox grant composition

- The launch's read/write roots must express: the repository root readable,
  the Git metadata readable but not writable, and the administration
  directory writable.
- **The protected-metadata model must not be widened.** If
  `discover_git_metadata` currently captures a whole `.git` path as one
  read-only unit, the implementation must find a composition that satisfies
  the grant *without* turning any other part of the Git metadata writable. If
  no such composition exists on this platform, the milestone **stops** — it
  does not widen the protected set.
- Any change to `src/security/sandbox/` must keep the existing negative
  guarantees intact: the read-root-nested-under-writable-root ambiguity
  rejection, the denied-path hardlink rejection, and the protected-tree
  symlink rejection.

### 6.4 Daemon wiring

- The daemon constructs the contained creator with its pinned Git binary and
  hands it to the worktree service.
- The branch is created before the contained child runs, through the existing
  typed `git branch` path. If the branch cannot be created, worktree creation
  is refused before any child is launched.

### 6.5 Documentation

- `architecture/git.md` — the contained worktree-creation route, its grant,
  and the fact that every other containment-requiring Git request remains
  unavailable.
- `architecture/session_launch.md` — update the "checkout may not be creatable
  on this host" section; it is no longer true on a qualified host.
- `architecture/linux_execution_isolation.md` (or the sandbox's owning doc) —
  the grant composition, if the sandbox model changed.
- The `git` skill's ownership map, if the module layout changed.

## 7. Ordered work packages

### WP1 — the seam in `codegg-core`

*Intent:* give `WorktreeService` a contained creation path without importing
process or sandbox types.*

Required changes: the trait, its request and outcome types, the optional
holder on `WorktreeService`, and the `create_reserved` branch that prefers it.

Acceptance evidence: a focused test that a `WorktreeService` built with a
contained creator calls the creator, and one that a service built without one
still fails closed with the existing typed error.

### WP2 — the contained executor in the root crate

*Intent:* the child, the containment, and the exact argv.*

Required changes: the module, the pinned binary, the launch, the
obtained-enforcement assertion, the environment, and the single argv shape.

Acceptance evidence: a test that the launch spec requests the M004 process/IPC
tier and the M005 offline network, that the obtained outcome is `Enforced`, and
that the argv is exactly `worktree add <path> <branch>` with no `-b`/`-B`.

### WP3 — the grant composition

*Intent:* the administration directory writable, the rest of the Git metadata
read-only, nothing else widened.*

Required changes: the read/write root composition; any sandbox-model change
that is strictly necessary, with the existing negative guarantees preserved.

Acceptance evidence: a structural assertion that the child's write roots
contain the administration directory and the new checkout, and do **not**
contain the repository's `refs`, `objects`, `index`, `config`, `hooks` or
`packed-refs`.

### WP4 — daemon wiring and the branch precondition

*Intent:* the production path creates the branch, then the worktree.*

Required changes: the daemon builds the creator; the branch is created before
the child launches; a failed branch creation refuses before any launch.

Acceptance evidence: a test that the branch exists before the child runs, and
that a branch-creation failure launches no child.

### WP5 — the hostile-configuration negative test

*Intent:* prove the sandbox, not the configuration, is the security property.*

Required changes: a fixture repository whose configuration defines a smudge
filter, a hook, a merge driver, an fsmonitor hook and a credential helper,
each of which attempts to write outside the grant and record that it ran.

Acceptance evidence: the worktree is created; every hostile hook's effect is
absent from the host; the child's writes stayed inside the grant. The checkout
itself is untrusted content — that is stated, not disproved.

### WP6 — the refusal paths

*Intent:* every failure is a typed refusal, and nothing degrades to an
uncontained attempt.*

Required changes: typed reasons for a host that cannot obtain enforcement, a
grant that cannot be composed, a branch that cannot be created, and a child
that fails.

Acceptance evidence: each refusal is distinguishable, and the reservation is
left orphaned with `git_error` health rather than reported as created.

### WP7 — closure

*Intent:* the evidence, honestly labelled.*

Required changes: the closure record; the registry and roadmap reconciliation;
the `architecture/` updates; the `git` skill update if the layout changed.

Acceptance evidence: §15 of this plan.

## 8. Failure, cancellation, restart and contention semantics

- A child that is cancelled or times out leaves the reservation orphaned with
  `git_error` health, exactly as today's failure path does. It never leaves a
  record claiming a checkout that does not exist.
- A child that fails after creating the administration directory leaves a
  partial worktree. The existing orphan/reconcile path owns it; the
  milestone must not add a second reconciliation authority.
- The worktree service's repository lock is already held across creation.
- Restart during creation is the orphan case: the reservation is not `ready`,
  so nothing binds to it.
- No new durable state, no new migration, no new scheduler interaction.

## 9. Compatibility and migration

- **No schema migration.** The record shape, states and lease model are
  unchanged.
- **No protocol or wire change.**
- Hosts that cannot obtain the M004/M005 tiers behave exactly as today: a
  typed refusal, no checkout.
- `egggit`'s classification of every other Git request is unchanged.

## 10. Required tests

Focused:

1. The seam: a service with a contained creator uses it; a service without one
   fails closed with the existing typed error.
2. The executor: the launch requests both tiers; the obtained outcome is
   asserted; the argv is exactly one shape and never contains `-b`/`-B`.
3. The grant: the write roots contain the administration directory and the new
   checkout, and contain none of `refs`, `objects`, `index`, `config`,
   `hooks`, `packed-refs`.
4. The branch precondition: the branch exists before the child runs; a failed
   branch creation launches no child.

Integration:

5. A managed worktree is created through the production path, is reported by
   `git worktree list` on the recorded branch, is a real linked worktree, and
   its files are Git-verified.
6. The hostile-configuration fixture: the worktree is created and no hostile
   hook's effect reaches the host.
7. The refusal paths: each is typed and distinguishable; no half-made checkout
   is reported as created.

Non-regression:

8. `git add`, `git commit`, `git merge`, `git cherry-pick`, `git rebase`,
   `git stash`, `git am`, `git bisect`, `git push` and `git pull` keep their
   current classification. This milestone must not make any of them available.
9. The existing `codegg-core` worktree and session-launch suites still pass.

## 11. Required verification commands

```bash
cargo fmt --all && cargo fmt --all -- --check
CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p codegg-core worktree
cargo test -p codegg --test <the new focused test target>
CARGO_BUILD_JOBS=2 bash scripts/verify.sh quick
```

The native Linux evidence (the hostile-configuration test and the obtained
enforcement) requires a **qualified host** — Landlock with the M004 process/IPC
tier and the M005 offline network. It must be run on the hosted Linux runner,
not claimed from a developer machine. The closure record labels local and
hosted evidence separately and truthfully.

## 12. Documentation updates

- `architecture/git.md` — the contained worktree-creation route.
- `architecture/session_launch.md` — the "not creatable on this host" section
  is now platform-dependent, not absolute.
- The sandbox's owning architecture doc, if the grant composition changed.
- `architecture/overview.md` — only if a module map row changed.
- `.opencode/skills/git/SKILL.md` — if the module layout changed.
- `plans/subsystems/linux-trusted-git-cost-corrective-roadmap.md` — the C008
  row and status.
- `plans/registry.md` — the C008 row and its unblock audit.

## 13. Acceptance criteria (externally observable)

1. `WorktreeService::create_reserved` succeeds on a qualified host and
   produces a real linked worktree reported on its recorded branch.
2. The child that produced it ran with the M004 process/IPC tier and the M005
   offline network, and the obtained enforcement was asserted after the
   launch.
3. A repository carrying a hostile smudge filter, hook, merge driver,
   fsmonitor hook and credential helper produces no host-side effect outside
   the grant.
4. The child cannot write the repository's refs, objects, index, config,
   hooks or packed-refs, and a request to do so fails rather than succeeding.
5. Every other containment-requiring Git request remains unavailable.
6. A host that cannot obtain enforcement refuses with a typed reason and
   reports no created checkout.

## 14. Stop conditions

The agent **reports rather than improvises** if any of these occur:

- The grant cannot be composed on the target platform without widening the
  protected Git metadata set. **Stop.** Do not widen the protected set; do
  not fall back to an uncontained child.
- Landlock or Seatbelt cannot represent "writable administration directory
  inside a read-only Git metadata tree". **Stop**, and report the platform
  limitation.
- The M004 process/IPC tier or the M005 offline network cannot be obtained on
  the qualified host. **Stop** and report which tier was missing.
- The hostile-configuration fixture shows a host-side effect outside the
  grant. **Stop.** That is a containment regression, not a test to relax.
- `git worktree add` proves to write a ref under the grant on the pinned Git
  version. **Stop** and report the observed write set.
- ADR-0048's clause 1 turns out to authorize something this implementation
  needs beyond the single operation shape. **Stop** and propose an ADR
  amendment rather than widening the implementation.

## 15. Closure evidence required

- The closure record at
  `plans/closure/linux-trusted-git-cost-corrective/008-status.md`, with the
  required sections.
- The requirement-to-evidence matrix mapping each acceptance criterion to a
  named test and its result, with local and hosted evidence labelled
  separately.
- The hostile-configuration evidence with the observed write set.
- The obtained-enforcement evidence for the production route.
- The non-regression evidence that every other containment-requiring Git
  request keeps its classification.
- An explicit statement of what is **not** claimed: no other Git operation, no
  mutable object reads, no metadata promotion, no Windows evidence, and the
  fact that a hostile filter's *output* is untrusted checkout content.
- The registry reconciliation, including the unblock audit: this closure
  unblocks the blackboard line's composed two-agent race milestone, which must
  be registered as `ready` in the same commit.

## 16. Handoff notes

- The authority is **ADR-0048**, not ADR-0044. ADR-0044 stays proposed.
- The template to copy is `src/git_publication/staging.rs`'s
  `ContainedGitIndexStaging`. Copy its *shape* — pinned binary, sanitized
  environment, tiers requested, obtained enforcement asserted after the
  launch — not its staging semantics.
- Two Git facts, both verified on this host against Git 2.43.0 and both
  load-bearing: a linked worktree's administration directory is always inside
  the common Git directory, and `git worktree add <path> <existing-branch>`
  writes no ref. If either stops holding on a different Git version, the
  branch-precondition design must be revisited — and that is a stop, not an
  improvisation.
- The blackboard line's composed two-agent race is the **next** milestone, not
  this one. Its plan is written against this milestone's shipped API.
- Windows produces no evidence here and claims none.
