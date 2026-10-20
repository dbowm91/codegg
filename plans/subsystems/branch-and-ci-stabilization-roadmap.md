# Branch and CI Stabilization Roadmap

Status: active; C001 closed

Repository planning baseline: `d251b11c` (branch tip before the rebase onto `main`)

Long-term references:

- `plans/000-long-term-specification.md` §4 (correctness before transparent magic);
- `plans/002-long-term-roadmap.md` — cross-phase execution rules;
- `plans/003-planning-process.md` §6 (authority order) and §7 (corrective passes);
- `architecture/testing.md` — the verification contract and CI policy;
- `architecture/overview.md` — the "Verified counts" table, the single source of truth for counts.

Related ADRs: none required. This workstream changes branch integration mechanics,
identifier hygiene, and test-suite determinism. It does not change product
authority, protocol semantics, storage ownership, or the runtime architecture.
The one accepted decision it *renumbers* — the command-authority ADR — keeps its
accepted status, date, and substance; only its canonical identifier moves, and
that move is recorded in `plans/adrs/linux-command-adr-id-migration.md` and in
`plans/implementation/linux-execution-isolation/012-adr-identity-collision-and-admission-gate-corrective.md`.

## 1. Purpose and ownership boundary

This workstream owns the **integrability** of a long-lived feature branch and the
**determinism** of the signal that branch's CI produces. It exists because a
branch that was developed across several `main` moves accumulates two kinds of
decay that no single feature milestone owns:

- **Identity decay** — two independently-numbered namespaces (storage-layout
  migrations, ADR identifiers) get allocated the same number on two branches,
  and the collision is only discovered when the branches meet.
- **Signal decay** — a test suite that reads a repository that Git itself is
  concurrently maintaining produces intermittent failures that look like
  product defects, and a suite that depends on a locally installed language
  server produces host-dependent timeouts.

The boundary is deliberately narrow. This workstream owns:

- the branch's integration with `main` (rebase, conflict resolution, and the
  identity collisions that resolution exposes);
- the **determinism** of the default test sweep — a green sweep must mean the
  product is green on any host, not on a host with a particular toolchain
  installed;
- the accuracy of the documented counts that the repository publishes.

It does **not** own feature behaviour, the Linux sandbox roadmap's milestones,
or the accepted status of any decision. Where this workstream renumbers an
identifier, the decision's substance is preserved and the rename is recorded.

## 2. Work classification

### Invariants

- Two active storage-layout migration numbers may never describe two different
  migrations, and `STORAGE_LAYOUT_VERSION` must track the highest wired
  migration.
- Two active ADR identifiers may never name two different decisions, and every
  live local ADR path must resolve.
- The default test sweep must not require a tool that the runner is not
  guaranteed to have.

### Capabilities

- A branch can be rebased onto `main` and remain buildable, testable, and
  internally consistent — with no silent content loss from either side.

### Infrastructure

- The published "Verified counts" table is re-measured against the tree it
  describes, and the method used is recorded.

### Polish

- Nothing in this workstream is a polish item. Every item here either restores
  an invariant or makes a failure mode honest.

## 3. Non-goals

- **Not** an admission decision. This workstream does not admit a host, does
  not change ADR-0044's status, and does not change the C010 no-admission
  disposition.
- **Not** a feature milestone. No new product capability is delivered.
- **Not** a release. No tag, no release, no publish.
- **Not** a change to the Linux sandbox roadmap's own milestones or blockers.
- **Not** an adjudication of the C005 plan, which remains blocked on owner
  decisions.
- **Not** a rewrite of historical closure evidence. Where a historical record
  carries a now-stale identifier, the identifier is corrected with a
  provenance note; the status, verdict, counts, and findings stay as they were.

## 4. Current state

At the planning baseline (`d251b11c`) the branch was 295 commits ahead of and
90 commits behind `main`, and it carried three defects that only `main`'s
recent work could expose:

1. **A storage-layout migration-number collision.** `main` allocated layout
   version 83 to the managed-agent-scratch migration; the branch allocated 83
   to Linux M005's network-intent preference and 84 to C007's private-index
   publication journal. Merging the two produced two functions named
   `migrate_v83` in one file — a tree that cannot compile.
2. **An ADR-identifier collision.** The branch's ADR-identity migration had
   read `main`'s allocation as ending at ADR-0041 and taken ADR-0042 for the
   accepted command-authority decision. `main` had since allocated ADR-0042 to
   the accepted composer normal-mode key ownership decision. `main` was clean;
   the branch was not.
3. **Content the branch had already lost.** The branch's own commit
   `c446af35` ("Close Linux execution isolation M004") had, as collateral
   damage, deleted eleven registry rows and their prose for three unrelated
   workstreams (TUI subagent and job visibility convergence, user prompt
   queue and turn contention, agent loop operational reliability). The
   deletion had been invisible because the rows were not re-read after that
   commit.

The branch's test signal had a second, independent defect: a private-object-view
construction path in `crates/egggit` reported a *transient* object-store read
failure as a typed "Git backend unavailable", so a concurrent `git gc` in the
same repository turned ordinary Git reads into test failures; and three LSP
tests that query a live language server sat at the nextest kill threshold
because their runtime is a workspace index, not a unit of work.

## 5. Target architecture

The target state is a branch that is (a) integrated with `main` at its current
tip, (b) internally consistent in both identifier namespaces, (c) free of
content that either side lost, and (d) verified by a sweep whose result
depends on the product and not on the host.

Concretely:

- **One namespace, one owner.** A storage-layout number is allocated once. If
  `main` already owns it, the branch's migration moves to the next free
  number; the branch never keeps a duplicate.
- **An ADR identifier is allocated once.** The same rule, plus: the identity
  guard is the authority, and it must pass before the branch can claim to be
  green.
- **A transient read is not a refusal.** If a Git object store changes while
  it is being read — the ordinary consequence of Git's own background
  maintenance — the read is retried inside the existing bound. Only a
  *persistent* failure is a refusal, and it stays a typed refusal.
- **The default sweep is host-independent.** A test that needs an installed
  language server is behind the `lsp-real-server-tests` feature, which the
  repository already defines for exactly this purpose.

## 6. Dependency graph

```text
        ┌──────────────────────────────────────────────┐
        │ main (fe2c1306)                              │
        │  · layout v83 = managed-agent scratch        │
        │  · ADR-0042 = composer normal-mode key       │
        └───────────────┬──────────────────────────────┘
                        │  identity collisions (layout, ADR)
                        ▼
   ┌────────────────────────────────────────────────────────────┐
   │ C001 — Branch and CI stabilization                          │
   │  A. integrate: rebase + resolve + repair                    │
   │  B. identity: migration numbers, ADR numbers, lost content  │
   │  C. signal: object-store instability, host-dependent tests  │
   │  D. documentation: re-measured counts                       │
   └───────────────┬────────────────────────────────────────────┘
                   │  produces
                   ▼
   ┌────────────────────────────────────────────────────────────┐
   │ the Linux sandbox workstream's next milestone (C005)         │
   │  — still blocked on its own owner decisions                 │
   └────────────────────────────────────────────────────────────┘
```

Hard dependency: C001 requires `main` to be reachable and to have been read
(the collision set is defined by `main`'s current allocation). No other
milestone depends on C001 for correctness; C001 exists so that the work that
*does* depend on a trustworthy branch can proceed.

## 7. Milestones

### C001 — Branch and CI stabilization

- **Class:** invariant (branch integration) + invariant (sweep determinism).
- **Objective:** bring the branch onto `main` without losing either side's
  content, and make the default sweep's green result mean what it claims.
- **Dependencies:** `main` reachable; no other milestone.
- **Deliverable boundary:** the branch's history, its two identifier
  namespaces, its published counts, and the determinism of its default test
  sweep.
- **Boundary correction — 2026-10-20 (recorded, not silent).** The boundary
  originally read "No product behaviour changes". The authoritative sweep showed
  that this milestone's exit condition ("the sweep's remaining failures are
  zero or explained") and this roadmap's completion definition ("the default
  sweep's result does not depend on the host") cannot both be met without one
  bounded product change: the scratch allocator's per-user cache root. The
  allocator created the root it then refused, so whether the product could run
  at all depended on the operator's umask. The change is confined to *how a
  cache root the product creates is created*; a cache root that already exists
  and another user could write to is still refused, with the same typed error.
- **Exit conditions:** the tree compiles with every target; both identifier
  guards pass; the canonical gate passes; the sweep's remaining failures are
  zero or explained; and the records say exactly what was and was not verified.

## 8. Cross-cutting requirements

- **Storage.** Every migration number is allocated once. `STORAGE_LAYOUT_VERSION`
  tracks the highest wired migration, and the catalog invariant proves it.
- **Identifiers.** Every active ADR identifier is unique and every live local
  ADR path resolves. Both are proved by `scripts/check_adr_integrity.py`, which
  is part of `scripts/verify.sh quick` and of the routine CI job.
- **Determinism.** A test in the default sweep may not depend on a host
  toolchain, on a repository's concurrent maintenance, or on another test's
  process-global state.
- **Honesty.** A failure that is recorded is recorded as a failure. A count
  is published only against a `Source` that yields it. Evidence that was not
  obtained is not reported as obtained.

## 9. Verification strategy

- The canonical gate: `scripts/verify.sh quick`.
- Compilation of every target: `cargo check --workspace --all-targets --locked`.
- The workspace sweep: `cargo nextest run --workspace --locked --profile ci`,
  run quiescent — never while editing files or running Git commands in the
  worktree.
- Each fix carries a regression test that is **provably non-vacuous**: the
  test must fail against the pre-fix behaviour, not merely pass against the
  post-fix behaviour.
- Both identifier guards are run directly, and the ADR guard's `--self-test`
  is run so the guard itself is not assumed to work.

## 10. Risks and decision points

- **Risk: a rebase silently drops content.** Mitigated by resolving conflicts
  explicitly rather than by a bulk strategy, by scanning every commit in the
  branch for conflict markers, and by diffing the tree before and after any
  history surgery.
- **Risk: a renumbering is read as a substantive change to an accepted
  decision.** Mitigated by keeping the decision's status, date, and text, and
  by recording the identity history in the migration record.
- **Decision point: which side wins a migration number.** Resolved in favour of
  `main`, because `main`'s number is already reachable by an existing
  database; keeping the branch's number would silently skip the branch's
  migration for anyone who had already run `main`'s.
- **Decision point: how to treat a transient object-store read failure.**
  Resolved in favour of retrying inside the existing bounded attempts, because
  the bound already exists for exactly this class of event and a persistent
  failure still refuses.
- **Decision point: what to do about a per-user cache root the product itself
  created.** The scratch allocator refuses a cache root that another local user
  could write to — a deliberate hardening (`c05ace49`, "validate per-user cache
  parent") that closes the "another user swaps the scratch root out from under
  the product" hole. The allocator nevertheless created that root with
  `create_dir_all`, whose mode follows the process umask, so on a host with
  umask 002 it created a 0775 root and then refused it. Resolved in favour of
  creating the root privately when the product creates it, and of **not**
  re-permissioning a root that already exists: the hardening stays exactly as
  it was, and only the self-defeating creation is removed.
- **Decision point: how the sweep gets a cache root of its own.** Resolved in
  favour of the canonical runners supplying one — `scripts/verify.sh full`
  creates a private directory with `mktemp -d`, and the CI job sets one under
  the runner's temporary directory — rather than a workspace-wide Cargo `[env]`
  override. The Cargo override would also have redirected the *product's* cache
  root for a local `cargo run`, which is a product behaviour change outside
  this milestone, and it would have placed the scratch root inside the
  repository, which several qualifications grant as their workspace root.
- **Decision point: what to do with the stray `plans/.mutate-probe`.** The
  parallel packet asks for its disposition and forbids guessing. Resolved in
  favour of removal, on evidence rather than on appearance: the file is 295
  lines of the single character `x`, it was added by a commit whose subject
  describes recording a sweep result, nothing in the repository reads it, and
  it is absent from `main`. The removal is recorded in the closure with that
  evidence, so a reviewer can check the reasoning rather than take it on
  trust.
- **Decision point: what to do with the two commits the remote branch carried
  past the rebase base.** Resolved in favour of replaying them onto the
  rebased branch rather than force-pushing over them. They are the owner's
  parallel planning packet, and a force push that dropped them would have
  destroyed work this milestone never saw.

## 11. Completion definition

This roadmap is complete when:

- the branch is rebased onto `main` and its history contains no conflict
  markers, no duplicate identifier allocations, and no content that either
  side lost;
- `scripts/verify.sh quick` passes;
- the default sweep's result does not depend on the host;
- every published count is measured against its listed `Source` on the tree
  that describes it.

## 12. Milestone status table

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| C001 | closed | `plans/implementation/branch-and-ci-stabilization/001-branch-and-ci-stabilization.md` | `plans/closure/branch-and-ci-stabilization/001-status.md` | none |
