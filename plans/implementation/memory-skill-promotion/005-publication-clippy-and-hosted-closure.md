# Memory-to-Skill Promotion Milestone 005 — Publication Clippy and Hosted Closure

Status: active

Repository baseline: `7ef387aa0302efa3106b1d14ee166fd93e921cb9`

Source corrective roadmap/addendum:

- `plans/subsystems/memory-skill-promotion-hosted-verification-corrective-addendum.md`

Predecessor evidence:

- M004 implementation: `plans/implementation/memory-skill-promotion/004-habit-lock-open-options-and-hosted-closure-corrective-pass.md`
- M004 closure: `plans/closure/memory-skill-promotion/004-status.md`
- Hosted failure: run `33836217483`, job `100909174354`, exact head `7ef387aa0302efa3106b1d14ee166fd93e921cb9`

Long-term requirements:

- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/003-planning-process.md#7-corrective-passes`

Applicable architecture:

- `architecture/memory.md`
- `architecture/skills.md`
- `architecture/tool.md`

Primary class: corrective reliability / verification / closure

## 1. Objective

Remove the six M002/M003-owned Clippy findings reported by the M004 exact-head
hosted run, while preserving the existing proposal and publication contracts,
then obtain green exact-head hosted verification through Workspace tests.

## 2. Why this milestone is ready

M004 landed its bounded habit lock correction. Its hosted candidate reached
Workspace Clippy and failed only on older memory-to-skill publication/proposal
code. The findings are localized, behavior-preserving corrections with no
unresolved architecture decision or external dependency.

## 3. Current implementation evidence

The hosted run reports:

- `src/skills/promotion.rs:417` — `clippy::too_many_arguments`;
- `src/skills/promotion.rs:625` — ambiguous lock-file open options;
- `src/skills/publish.rs:97` and `:138` — ambiguous publication lock-file open options;
- `src/skills/publish.rs:210` — `clippy::too_many_arguments`;
- `src/skills/publish.rs:422` — `clippy::needless_borrows_for_generic_args`.

All findings predate M004 and are retained as historical M002/M003 evidence;
M005 owns only their corrective implementation and hosted closure.

## 4. Invariants that must not regress

- Proposal submission remains explicitly user-triggered and model-inaccessible
  for approval/publication.
- Publication remains host-owned, CodeGG-rooted, path-safe, atomic, durable,
  collision-safe, and digest/revision-bound.
- Proposal and publication lock files are synchronization artifacts and are
  never truncated before advisory ownership is acquired.
- Existing proposal schemas, skill files, roots, parser rules, provenance,
  reconciliation, and refresh semantics remain unchanged.

## 5. Scope

In scope:

- explicit non-truncating open policy for the three proposal/publication lock
  opens;
- behavior-preserving argument bundling or equivalent local refactors for the
  two functions exceeding Clippy’s argument limit;
- removal of the needless borrow;
- focused regression verification and exact-head hosted closure evidence.

Out of scope:

- proposal/publication feature redesign or schema changes;
- habit observation, habit thresholds, or the M004 habit lock helper;
- parser, collision, path, permission, refresh, or asset-registry redesign;
- Clippy suppressions, CI weakening, toolchain pinning, or a new CI lane.

## 6. Required production changes

Use small private request/context structs or equivalent same-module helpers to
reduce argument counts without changing call ordering, lock scope, error
mapping, or ownership. Add `.truncate(false)` to each synchronization lock
open. Remove only the unnecessary borrow at the reported path join.

## 7. Failure, cancellation, restart, and contention semantics

Preserve current failure and cleanup behavior. Lock acquisition still occurs
before load/mutate/save or publication/reconciliation work; errors still
release the advisory lock through the existing cleanup path; concurrent
writers continue to serialize on the same lock paths.

## 8. Compatibility and migration

No storage migration, protocol change, file-format change, root-path change,
or compatibility action is permitted.

## 9. Required tests

- `cargo test -p codegg-core habit --locked`;
- `cargo test -p codegg-core memory --locked`;
- `cargo test --test habit_skill_promotion --test skill_publication --locked`;
- existing publication/proposal unit and integration tests affected by any
  refactor;
- workspace Clippy and quick verification.

## 10. Required verification commands

```bash
cargo fmt --all -- --check
git diff --check
cargo test -p codegg-core habit --locked
cargo test -p codegg-core memory --locked
cargo test --test habit_skill_promotion --test skill_publication --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
```

The closure record must identify any local architecture/toolchain workaround
and must cite one normal hosted `CI / verify` run on the exact accepted SHA
that passes Workspace Clippy and reaches/passes Workspace tests.

## 11. Acceptance criteria

1. All six hosted findings are corrected without suppressions.
2. Proposal/publication behavior and lock lifecycle remain unchanged.
3. Focused tests and quick verification pass.
4. Exact workspace/all-target Clippy passes locally.
5. Existing hosted `CI / verify` passes on the exact accepted candidate
   through Workspace tests.
6. A closure record and registry update reconcile M004/M005 history without
   rewriting M001–M004 records.

## 12. Stop conditions

Stop and register a separate follow-up if fixing these findings requires a
schema, authority, filesystem-root, persistence, protocol, or CI architecture
change, or if hosted CI fails on a new finding outside this scope.

## 13. Closure evidence required

Create `plans/closure/memory-skill-promotion/005-status.md` with the
requirement-to-evidence matrix, exact files/commits, focused and local
verification, hosted run/job through Workspace tests, invariant/security/
compatibility review, unresolved findings, and the final downstream unblock
audit.
