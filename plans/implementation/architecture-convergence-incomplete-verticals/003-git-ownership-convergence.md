# Architecture Convergence M003 — Git Ownership Convergence

Status: implemented

Repository baseline: `3c4890035513cd4d74430b6f64523c8be676024e`

Source subsystem roadmap:

- `plans/subsystems/architecture-convergence-incomplete-verticals-roadmap.md`

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#4.2-explicit-ownership`
- `plans/000-long-term-specification.md#4.7-correctness-before-transparent-magic`
- `plans/000-long-term-specification.md#9-project-repository-workspace-and-worktree-model`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/003-planning-process.md`

Relevant dependencies:

- agent-run/worktree M009 closed;
- runtime safety edit-history M013 closed;
- previous Git security/polish validation remains accepted historical evidence;
- no hard dependency on other milestones in this roadmap.

Primary class: invariant / infrastructure

## 1. Objective

Clarify and enforce the ownership boundary among `egggit`, `codegg-git`, and root Git adapters so generic Git safety/domain primitives, CodeGG workflow orchestration, and UI/tool projection do not evolve as parallel implementations.

Target ownership:

```text
egggit
  generic Git command/domain primitives
  safe argv/environment/process conventions
  repository/object/reference inspection

codegg-git
  CodeGG worktree/run/mutation orchestration
  provenance and integration contracts
  durable rerun/integration helpers where Git-aware

root
  model-facing tools
  TUI command adapters
  projection/diagnostic mapping
```

Exact crate names may remain unchanged; semantic ownership is the requirement.

## 2. Explicit non-goals

M003 must not:

- replace Git with libgit2/gix as a broad rewrite;
- merge `egggit` and `codegg-git` solely because two crates exist;
- preserve both crates if one is demonstrably only a forwarding layer;
- redesign worktree identity, scheduler contention keys, or agent-run lineage;
- add automatic merging/rebasing;
- weaken hook/network/credential protections;
- make rerun itself work; M005 owns the user-visible rerun vertical.

## 3. Current implementation evidence to inspect

Inspect at least:

- `crates/egggit/`;
- `crates/codegg-git/`;
- root Git-facing tool modules including commit/diff/apply/edit integration;
- `src/git_mutation_projector.rs`;
- worktree service/orchestration code;
- RunStore Git provenance/rerun metadata;
- Git validation docs and forbidden-pattern guards;
- environment/credential handling for authenticated remotes;
- any direct raw `git` process invocation outside the two Git owners.

For every Git operation category, identify one canonical domain owner and one or more adapters.

## 4. Required ownership contract

`egggit` should own reusable Git mechanics that do not depend on CodeGG session/run/job/project types. `codegg-git` should own workflows that do depend on CodeGG durable identity or lifecycle. Root code should not duplicate either category.

At minimum classify ownership for:

- repository discovery/identity inspection;
- status/diff/log/ref/object reads;
- safe Git subprocess construction;
- credential-safe remote invocation;
- commit creation;
- worktree create/remove/list/validate;
- branch/base/result commit relationships;
- mutation provenance;
- run rerun Git reconstruction;
- integration/promote operations;
- projection-safe mutation summaries.

## 5. Ordered work packages

### WP1 — Operation matrix

Build a matrix of Git operations, current implementations, production callers, and selected owner. Highlight forwarding-only APIs and duplicated safety logic.

### WP2 — Generic primitive convergence

Move or consolidate generic Git logic into `egggit`. Ensure no CodeGG-specific durable types leak downward merely to justify a crate boundary.

### WP3 — CodeGG workflow convergence

Move CodeGG-specific worktree/run/mutation/integration rules into `codegg-git` where they can be consumed by agent tools, scheduler/worktree services, and rerun logic without root duplication.

### WP4 — Root adapter cleanup

Reduce root Git modules to authorization/tool schema/result mapping/projection concerns. Delete raw Git invocation and provenance reconstruction duplicated from the canonical crates.

### WP5 — Decide whether both crates remain justified

After migration, evaluate whether `codegg-git` still owns enough CodeGG-specific workflow to justify a distinct crate. If it is only a forwarding facade, collapse it into the appropriate owner and update workspace dependencies. If both remain, document the distinction in crate-level docs and architecture docs.

This decision is evidence-driven and does not itself require an ADR because it is an internal ownership cleanup unless a public crate contract has been intentionally guaranteed elsewhere.

### WP6 — Guards and docs

Update existing Git forbidden-pattern/architecture guards to prevent new raw Git subprocess or secret-bearing argv paths outside the canonical owner, using narrow allowlists only where justified.

Update Git architecture documentation with the final crate/root ownership map.

## 6. Storage, protocol, migration, compatibility

No durable schema change is expected. Historical run/mutation records must remain readable. Internal type moves may require serde/import compatibility but must not rewrite stored provenance.

No frontend protocol change is expected.

## 7. Security and failure semantics

Preserve:

- hook-free/network-free behavior for local inspection where currently required;
- credential reacquisition rather than durable raw secret storage;
- redacted argv/projection behavior;
- explicit base/result commit validation;
- generation-fenced worktree leases;
- repository/workspace containment checks;
- conflict/dirty-state fail-closed semantics.

## 8. Verification

Run focused `egggit`, `codegg-git`, worktree, mutation, and Git security tests plus:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Do not add new cross-platform matrices or security scanners.

## 9. Acceptance criteria

M003 is complete only when:

- every major Git operation has one documented owner;
- generic Git mechanics and CodeGG workflow semantics are not duplicated across crates/root;
- raw root Git execution is eliminated or explicitly justified;
- the continued existence or collapse of `codegg-git` is evidence-backed;
- worktree/run/mutation security invariants remain covered;
- M005 can consume one stable Git workflow boundary for rerun;
- focused and quick verification pass.

## 10. Stop conditions

Stop if the cleanup requires changing durable worktree identity, scheduler authority, project authorization, or public Git protocol semantics. Those require separate architecture work.

## 11. Closure evidence required

The closure record must include:

- operation/owner matrix;
- implementation commits;
- final crate/root ownership diagram;
- deleted forwarding/duplicate paths;
- security guard/test outcomes;
- compatibility findings;
- verification outcomes;
- explicit readiness statement for M005.
