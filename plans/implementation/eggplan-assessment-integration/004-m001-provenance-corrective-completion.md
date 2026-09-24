# Eggplan Assessment Integration Milestone M004 — M001 Provenance Corrective Completion

Status: blocked
Repository baseline: `edcfd0d` (M001 partial implementation)

Source roadmap: `plans/subsystems/eggplan-assessment-integration-roadmap.md`
Long-term requirements: `plans/000-long-term-specification.md`; `plans/001-terminology-and-domain-model.md`; `plans/002-long-term-roadmap.md`
Applicable ADRs: None required; this completes the already accepted attempt authority boundary.
Primary class: invariant / infrastructure

## 1. Objective

Complete the correctness requirements left open by M001's corrective-pass
closure so its durable execution subject is valid for every supported
evidence-relevant execution boundary and can unblock Eggplan M002.

## 2. Why this milestone is ready

The CodeGG-native subject DTO, nullable JobAttempt storage, local scheduler
capture/seal, and first enriched resolver exist at `edcfd0d`. This handoff is
blocked until Eggwork C001 supplies a stable accepted-input/materialization
contract and the coordinated Eggplan M002 bridge/head is rechecked.

## 3. Current implementation evidence

M001 closure record: `plans/closure/eggplan-assessment-integration/001-status.md`.
It records the implemented local path and the missing remote, RunStore,
AgentRun, submodule, and cross-repository evidence.

## 4. Invariants

- JobAttempt remains the sole historical execution-subject authority.
- Legacy NULL provenance is never backfilled from current workspace state.
- Capture remains bounded and owned by egggit / scheduler canonical-root seams.
- Local and immutable remote input seal points remain distinct.
- RunManifest and AgentRun subject values are correlated projections only.
- M002 remains blocked until every M001 acceptance criterion has evidence.

## 5. Scope

In scope: Eggwork accepted-input sealing; restart-stable RunManifest projection;
exact AgentRun job+attempt resolution; full bounded path/index/content and
recursive submodule capture; typed bound/unsafe failure coverage; JobStore
round-trip/CAS/retry tests; historical no-fallback guards; reviewed Eggplan
bridge fixtures and current upstream head recheck.

Out of scope: Eggplan production assessor adoption (M002), repository Plan
binding (M003), and changes to scheduler/worktree ownership.

## 6. Required production changes

Implement only after Eggwork C001's accepted-input contract is stable. Carry
the seal operation through the scheduler-owned authority; do not make an
executor an independent subject authority. Add optional bounded provenance to
RunManifest and resolve AgentRun exclusively through its exact durable
job+attempt link. Complete capture's path and submodule bounds, preserving
typed unavailability on any failure.

## 7. Ordered work packages

1. Freeze the Eggwork input-seal interface and capture S1/S2 around actual
   materialization, before remote submit.
2. Persist and reload correlated RunManifest provenance; assert equality with
   the linked attempt.
3. Resolve AgentRun by exact job and attempt; missing/dangling links stay
   unavailable.
4. Complete canonical manifest hashing for index state, symlinks, deletions,
   untracked files, and bounded recursive submodules.
5. Add persistence, retry, restart, drift, migration, resolver, and static
   ownership regression coverage.
6. Recheck Eggplan repository heads and prove conversion using reviewed golden
   fixtures without a production eggplan-repo dependency.

## 8. Failure, cancellation, restart, contention semantics

Started-only provenance after a crash remains non-evidence. A failed capture
or seal remains unavailable. Local drift is Drifted. Once an immutable remote
input is sealed, later local edits do not rewrite its historical subject.
Conflicting writes and terminal reseals fail closed.

## 9. Compatibility and migration

Additive/defaulted RunManifest fields only; old manifest files remain valid.
JobAttempt v67 NULL rows remain legacy unavailable. No backfill or old-manifest
rewrite is permitted.

## 10. Required tests

Capture fixtures for every required path state, symlink, submodule, and bound
failure; in-memory/SQLite/restart/CAS/retry tests; local and remote seal-point
tests; RunStore restart and exact AgentRun link tests; legacy resolver tests;
and Eggplan conversion goldens.

## 11. Required verification commands

Run the M001 focused commands and `./scripts/verify.sh quick`, workspace
Clippy, core-boundary and execution-ownership guards, plus hosted CI. Record
actual results in the M004 closure record.

## 12. Documentation updates

Update `architecture/jobs.md`, `architecture/run_store.md`,
`architecture/work_plan.md`, `architecture/git.md`, this roadmap, and
`plans/registry.md` with the qualified remote seal and resolver contracts.

## 13. Acceptance criteria

Every M001 acceptance criterion has positive local and hosted evidence;
historical resolution requires no current-worktree reads; Eggplan M002's only
CodeGG blocker is removed by an accepted M004 closure.

## 14. Stop conditions

Stop if Eggwork cannot provide an immutable materialization boundary, if any
supported evidence's verification specification cannot be reconstructed, or
if conversion is lossy against the current Eggplan contract.

## 15. Closure evidence required

Create `plans/closure/eggplan-assessment-integration/004-status.md` with a
requirement matrix, test/guard/hosted CI results, current Eggplan commit heads,
and explicit M002 unblock decision.

## 16. Handoff notes

Do not start M002 adoption until this corrective completion is positively
closed and Eggplan's staged-adoption plan is revalidated against current heads.
