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
- **Fresh rebase — 2026-10-21 (recorded, not silent).** `main` advanced by 103
  commits while this milestone was in flight, and the branch was rebased again
  onto `main` at `cb0e79c0` (304 commits replayed; the merge base moved from
  `fe2c1306`). That rebase closed a **second** ADR collision (the branch's
  Linux execution isolation decision moves to **ADR-0046**, because `main` owns
  ADR-0043), proved that no file's content was lost (the canonical per-file
  three-way merge matches the rebased tree byte-for-byte across 93 files with 0
  deviations, and the tree holds zero conflict markers), and exposed three
  cross-file pairings that no two-way merge can see — a file `main` added
  against a type this branch extended. Those are closed, the tree compiles with
  every target, and the feature-enabled tier is green at 12474/12474 after an
  inherited race in one of `main`'s tests was fixed. See §3.12–§3.15 of the
  closure.
- **Reconciliation with `main` at `2a0bae08f` — 2026-10-21 (recorded, not
  silent).** `main` advanced a further **9 commits** after the fresh rebase's
  base, and the pull request was reported `CONFLICTING`. The branch was
  reconciled with it by a **merge** — the pattern this repository's own history
  already uses for a branch carrying work `main` has not seen — and the
  tip-level conflict was **exactly one file**, resolved by the rule that a
  later qualification supersedes an earlier deferral. The branch now carries
  `main`'s tip as a direct ancestor, the merged tree holds zero conflict
  markers, every target compiles, the canonical gate passes, and **no
  published count moved**: `main`'s nine commits added and deleted no file
  under any directory a count is measured from. The reconciled tree's sweep is
  green at **15755/15755** and its feature-enabled tier at **12496/12496**,
  with the live Eggwork qualification **16/16** on its own. See §3.16 of the
  closure.
- **Reconciliation with `main` at `84aa47528` — 2026-10-21 (recorded, not
  silent).** `main` advanced again — PR #151, the M006 scratch usage census —
  and the pull request was `CONFLICTING` again. Reconciled by a second
  **merge**, for the same reason as the first. This advance is **not**
  count-neutral: it added a `CoreRequest` variant, a `CoreResponse` variant,
  an authorized operation, and an integration test file, so **four** published
  rows moved and were re-derived against their own listed `Source` —
  `CoreRequest` variants 260 → **261** (237 struct + 24 unit), `CoreResponse`
  variants 169 → **170**, authorized operations 244 → **245**, integration test
  files 284 → **285**. The enum-variant measure was **calibrated** against the
  pre-merge tree's published values before it was applied to the merged tree.
  The twice-merged tree's sweep is green at **15771/15771** and its
  feature-enabled tier at **12512/12512**. See §3.17 of the closure.
- **Reconciliation with `main` at `28724bd83` — 2026-10-21 (recorded, not
  silent).** `main` advanced again — PR #152, the M007 scratch retention sweep —
  and the pull request was `CONFLICTING` again. Reconciled by a third **merge**.
  This is the first reconciliation whose conflict set reaches **product code**:
  `src/scratch.rs` conflicted, and the resolution is a three-part union — the
  branch's `current_owner_uid()`, `main`'s `scratch_cache_root()` resolver, and
  the branch's `ensure_private_cache_root(...)` private creation — because
  dropping the branch's part would put `main`'s umask-following creation back.
  This advance is **not** count-neutral: **four** published rows moved and were
  re-derived against their own listed `Source` — `CoreRequest` variants 261 →
  **262** (238 struct + 24 unit), `CoreResponse` variants 170 → **171**,
  authorized operations 245 → **246**, integration test files 285 → **286**. The
  merge also created a **cross-file pairing no file comparison can see**:
  `main`'s new `src/core/scratch_sweep.rs` builds a `JobPayload::Test` without
  the `sandbox_profile` and `network_intent` fields this branch added, so the
  merged tree did not compile until `842665094` filled them. See §3.19 of the
  closure.
- **Reconciliation with `main` at `c93211542` — 2026-10-21 (recorded, not
  silent).** `main` advanced again — PR #153, the TUI subagent presentation and
  name-allocation workstream (P001–P004) — and the pull request was
  `CONFLICTING` again. Reconciled by a fourth **merge**. This is the first
  reconciliation whose conflict set contains **no product code at all**:
  `plans/registry.md` was the only conflicted file, and both sides' content is
  disjoint rows, so the resolution is a union. **No source file was changed by
  both sides** — the intersection of the two sides' changed-file sets is
  `architecture/tui.md`, `docs/tui.md`, and `plans/registry.md`, all
  documentation — so this merge introduced **no new cross-file pairing**. This
  advance **is** count-neutral: the calibrated measure reproduces 262 / 171 /
  246 / 286 exactly, so no published row moved. See §3.20 of the closure.
- **Reconciliation with `main` at `239f06e7f` — 2026-10-21 (recorded, not
  silent).** `main` advanced again — PR #154, the M006 scratch census's tri-state
  reclaimable verdict — and it advanced onto **this branch's own scratch surface**.
  Reconciled by a fifth **merge**. The merge was **textually clean and that was not
  sufficient**: `main` moves the verdict from `bool` to `Option<bool>` across the
  domain type and the protocol DTO, while the **mapping site in
  `src/core/daemon_scratch.rs` is not in `main`'s changed-file set** — the two ends
  of the assignment moved and the middle did not. That is the cross-file pairing
  class a file-level proof cannot see, and the **compile-level proof settled it**:
  zero conflicts and `cargo check --workspace --all-targets --locked` → **exit 0**,
  with the branch's own `current_owner_uid()` and `ensure_private_cache_root(...)`
  intact. The advance is **count-neutral** — the calibrated measure reproduces
  262 / 171 / 246 / 286 exactly. See §3.21 of the closure.
- **The verification plan was pruned, deliberately and on the record — 2026-10-21.**
  This linked worktree's build cache was **cold** at the start of the fourth
  reconciliation, so a local full-workspace sweep means compiling and linking
  roughly 290 integration test binaries from scratch; the earlier attempt on
  the thrice-merged tree was killed with **signal 9** during `rustc` codegen
  (Cargo exit 101) — a host-resource kill during compilation, not a test
  failure. `.github/workflows/ci.yml` runs
  `cargo nextest run --workspace --locked --profile ci` on the pull request's
  own head, on dedicated runners, as the pull request's own required check — so
  the hosted run is the **authoritative** full-suite evidence and is strictly
  better than a local copy of it. The local suite was narrowed to the changed
  surface: the `codegg` library test binary, 13 integration binaries each
  mapping to a file this branch changed, and the four crates this branch
  changed in full. **The full workspace sweep was not run locally on this
  tree**, and the closure says so. See §3.20.1 of the closure.
- **Hosted evidence — 2026-10-21 (recorded, not silent).** The default sweep
  and the five native workflows ran on the rebased head and **concluded
  green**: the hosted default sweep reports **15733 tests run: 15733 passed, 8
  skipped** — the same count the local sweep reports — and the M004/M005
  native tier is qualified on a Landlock-ABI-6 host this workstation does not
  have. The same was re-observed on the head after the first reconciliation
  (**15755 run, 15755 passed, 8 skipped**, and all five native workflows
  **success**). The `pull_request` event created no run on the first two
  heads because a `pull_request` workflow checks out the synthetic
  `refs/pull/N/merge` ref, and GitHub cannot create that ref for a
  **conflicting** pull request; on the head that carries `main`'s tip the ref
  exists and the event fires for the whole set of workflows, `CI` included.
  The reconciliation with `main` is therefore a prerequisite for the hosted
  evidence, not merely a tidiness step. See §4.3 of the closure.
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
- The full-workspace sweep is obtained on **hosted CI**, on dedicated runners,
  for the reasons §7 records; locally the suite is narrowed to the changed
  surface. A local run and a hosted run are never reported as each other.
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
- **Decision point (2026-10-21): which side wins an ADR number, when the
  collision appears only after a later rebase.** Resolved the same way —
  `main` wins, and the branch's decision moves to a free number. The second
  collision was **ADR-0043**: `main` allocated it to the accepted
  session-owned subagent lifecycle projection decision while this branch had
  published it for the accepted composable Linux execution isolation decision.
  The branch's decision therefore moves to **ADR-0046**, keeping its
  substance, its accepted status, and its acceptance date. The rule is applied
  uniformly rather than case by case, and the ADR guard's self-test now carries
  a non-vacuous case for exactly this collision class, so the renumbering is
  enforced rather than merely asserted.
- **Decision point (2026-10-21): what counts as proof that a rebase lost
  nothing.** Two proofs are required, because they answer different questions
  and neither substitutes for the other. A **file-level** proof — the
  canonical per-file three-way merge of (branch, merge base, `main`) compared
  with the rebased tree — establishes that no *content* was dropped. A
  **compile-level** proof establishes that the *result* is coherent, and it
  covers the class a file-level proof cannot see: a pairing between a file
  one side added and a type the other side extended. The fresh rebase needed
  both: the file-level proof was clean (93 files, 0 deviations) and the tree
  still did not compile, because three such pairings existed.
- **Decision point (2026-10-21): merge or rebase, when `main` advances again
  after the branch has been rebased.** Resolved in favour of a **merge**. The
  repository's own history carries four `Merge origin/main into <branch>`
  commits for this situation, and two workstreams in this repository recorded
  their reconciliations the same way; a third rebase would have replayed 309
  commits to resolve one file and would have invalidated the hosted evidence
  already obtained on the pre-merge head. The branch now carries `main`'s tip
  as a direct ancestor.
- **Decision point (2026-10-21): which side wins a conflict where one side's
  later work discharges the other side's stated precondition.** Resolved in
  favour of the **later qualification**: `main`'s M007 qualified the
  lease-aware reclamation this branch's skill file was waiting on, so the
  branch's "cleanup intentionally retains these resources until lease-aware
  reclamation is qualified" yields to `main`'s description of the qualified
  behaviour. This is the same shape as the ADR rule in a different domain —
  where one side's later work discharges the other's precondition, the
  discharged side yields — and the branch's own M005/M004 text, which `main`
  does not carry at all, is kept in full.
- **Decision point (2026-10-21): how to obtain the hosted evidence when the
  `pull_request` event produces no run.** On the first two heads it produced
  none, and the cause is **not** in the repository's workflow file: `ci.yml`'s
  trigger block is byte-identical to `main`'s and the `verify` job has no `if:`
  condition. The cause is that a `pull_request` workflow checks out the
  synthetic `refs/pull/N/merge` ref, and **GitHub cannot create that ref for a
  conflicting pull request** — there is no merge commit to check out. Both
  earlier heads conflicted; on the head that carries `main`'s tip the ref
  exists (`c9da671b…`, a synthetic merge into `84aa47528`) and the event fires
  for the whole set of workflows, `CI` included. The consequence is an
  ordering rule rather than a workaround: **the reconciliation with `main` is
  a prerequisite for the hosted evidence, not merely a tidiness step**, and a
  `workflow_dispatch` against the branch head remains the way to obtain the
  evidence while a pull request still conflicts.
- **Decision point (2026-10-21): where the full workspace sweep runs.** The
  branch's local worktree has a **cold** build cache, so a local full-workspace
  sweep means compiling and linking roughly 290 integration test binaries from
  scratch — the milestone's largest time cost, and one that was killed by the
  host during compilation. `.github/workflows/ci.yml` already runs exactly that
  command on the pull request's own head, on dedicated runners, as the pull
  request's own required check. Resolved in favour of treating the **hosted run
  as the authoritative full-suite evidence** and narrowing the local suite to
  the changed surface — the `codegg` library test binary, 13 integration
  binaries each mapping to a file this branch changed, and the four crates this
  branch changed in full. This does not weaken any gate: the hosted job runs
  the same command `scripts/verify.sh full` runs, over the same workspace, on
  hardware this milestone does not share. The give-up is recorded rather than
  hidden: the closure states that the full sweep was not run locally on this
  tree, and the claim for this head rests on the hosted run.
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
- **Decision point: whether a live qualification's trust chain may follow the
  operator's umask.** The feature-enabled tier failed five live Eggwork
  qualifications on a tree, a fixture, and a code base identical to a green
  run. The cause was the *fixture*, not the product: the pinned node refuses a
  sandbox helper it cannot trust, and neither cargo nor `tempfile` narrows the
  modes they create, so on a host whose umask is `0002` the helper was
  installed group-writable and the node dropped
  `isolation.landlock.workspace-rw.v1`. Resolved in favour of **staging** the
  node and the helper in a private directory whose modes the qualification
  sets itself, and of changing **nothing** in the upstream refusal: the node
  still refuses an untrusted helper, and the product's executor still refuses a
  node that cannot isolate the workspace. The alternative — narrowing the
  upstream check, or trusting the build artifact as it stands — would have
  weakened a security boundary to make a test pass, and is refused. The
  non-vacuity is proved in the qualification itself: with the build tree
  restored to the failing state the same binary goes from 8 passed / 5 failed
  to 14/14.

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
| C001 | closed | `plans/implementation/branch-and-ci-stabilization/001-branch-and-ci-stabilization.md` | `plans/closure/branch-and-ci-stabilization/001-status.md` | none (the hosted CI run on the rebased head has **landed green** — 15733 run, 15733 passed, 8 skipped, re-observed green at 15755/15755 on the first reconciled head, at 15771/15771 on the twice-merged head, and at **15841 run, 15841 passed, 8 skipped** on both the four-times-merged head (`39632391908`) and the final five-times-merged head `4e4023ed8` (`39642334455`), with all seven native workflows **success** on it; the branch is reconciled with `main` at `239f06e7f` and carries its tip as a direct ancestor, through five merges — `2a0bae08f`, `84aa47528`, `28724bd83`, `c93211542`, `239f06e7f`) | |
