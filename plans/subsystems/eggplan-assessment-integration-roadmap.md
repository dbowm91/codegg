# Eggplan Assessment Integration Roadmap

Status: active corrective; M001/M002/M003 closed, C001 dirty-subject corrective ready

Canonical authority:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`
- `plans/subsystems/long-horizon-work-execution-roadmap.md`

External assessment substrate:

- Eggplan repository: `https://github.com/eggstack/eggplan`
- Eggplan M002 pure bridge implementation:
  `088968bd58680ae2b3741e2f1feb0614e0ff81a0`
- Eggplan M003 implementation pinned by CodeGG:
  `3f7c603315131bb169bfdd2bb575531d228532b1`
- Eggplan M003 closure reconciliation:
  `6644725ae540b19873d7c769e3afeab7bce53d09`
- Eggplan M003 C001 corrective planning registration:
  `ee92edc1cf035010fe86ea2382694cc18a0dce45`
- Eggplan C001 plan:
  `plans/implementation/codegg-integration/003-c001-dirty-subject-fingerprint-and-bound-evidence-requalification.md`
- current CodeGG planning baseline:
  `ffbd0bc9de09055fbd2df293a6cdd8b4b04a8e98`
- current CodeGG storage layout: v68 (M003 additive)
- current pinned Eggwork revision:
  `6cc813418c3f14740a635fef79208e85219175bb`

Implementation MUST re-check both repository heads before each cross-repository
handoff.

## 1. Purpose

Adopt Eggplan's generic plan/evidence assessment semantics inside CodeGG
without moving CodeGG WorkPlan persistence, Goal/Todo/checkpoint state,
scheduler authority, worktree ownership, or agent-loop policy into Eggplan.

The first required CodeGG primitive is durable execution-subject provenance.
A completed job/run cannot become exact-subject evidence if CodeGG records only
its terminal status while the worktree may have changed since execution.

## 2. Ownership boundary

CodeGG remains authoritative for:

- WorkPlan/WorkItem SQLite state and CAS;
- JobRecord/JobAttempt lifecycle;
- AgentRun lifecycle;
- RunStore records/artifacts;
- workspace/worktree identity and leases;
- scheduler admission/retry/cancellation;
- Goal/Todo/checkpoint/context-epoch behavior;
- completion-arbiter control flow;
- capture of execution-time source provenance.

Eggplan may later consume a pure snapshot containing:

- plan intent;
- exact source subject;
- host-resolved evidence status;
- authoritative verification binding;
- explicit provider policy.

Eggplan does not become the CodeGG scheduler, JobStore, RunStore, Git owner, or
worktree manager.

## 3. Core invariants

1. Historical execution provenance is recorded at execution time, never
   reconstructed from the current worktree after the fact.
2. Source provenance belongs to an execution attempt, not the logical job:
   retries may observe different source state.
3. Missing legacy provenance remains unavailable; migration never invents it.
4. A terminal job status alone is not exact-subject proof.
5. Caller/model prose, WorkPlan acceptance text, owner IDs, and labels cannot
   manufacture a subject.
6. CodeGG captures source state through its governed Git/workspace owners; no
   new raw `std::process::Command` Git owner is introduced.
7. Subject capture is bounded and path-safe; persisted provenance contains
   stable IDs/digests, not source contents or credentials.
8. Local live-workspace execution that observes source drift cannot be
   promoted to stable exact-subject evidence.
9. Snapshot/materialized remote execution binds to the source state that was
   sealed into the remote input; later local edits do not rewrite that
   historical provenance.
10. Eggplan adoption must remain fail-closed when subject or verification
    identity is absent or mismatched.

## 4. Milestones

### M001 — Durable execution-subject provenance

Status: closed.

Plan:

- `plans/implementation/eggplan-assessment-integration/001-durable-execution-subject-provenance.md`

The handoff was revalidated after Eggwork M002/M002a closed. M001 closed at
CodeGG `418fdc85656e7e1faa57f71e5e7f10f7f4859c60`; closure evidence is
`plans/closure/eggplan-assessment-integration/001-status.md` and hosted `CI`
run `36106606574` succeeded. Follow-up corrective C002
(`plans/closure/eggplan-assessment-integration/002-status.md`) repaired the
AgentRun link predicate, pinned the bridge projection with golden tests,
and added the subject-ownership guard; no closed API changed. The plan pins the v66 -> v67 storage migration
and actual Eggwork execution-input boundary:

    capture S1
      -> build immutable in-memory WorkspaceSnapshot
      -> capture S2 + manifest digest/completeness
      -> upload copied bytes
      -> create remote workspace
      -> execute

Exact-subject remote evidence is available only when S1 == S2 and the snapshot
is complete under the current transfer contract. Current snapshot omissions
(symlink/non-regular/oversize) remain explicit
`MaterializationIncomplete` provenance rather than silently becoming exact
proof.

Capture a versioned CodeGG-native source subject at the authoritative execution
input boundary, persist it on JobAttempt, make it available to linked
AgentRun/RunStore evidence resolution, and expose a bounded enriched
host-evidence resolver. Legacy records remain subject-unavailable.

Exit condition: a completed current-generation CodeGG job can resolve its
historical execution subject after restart without consulting the current
worktree, while legacy/missing/drifted/incomplete-materialization cases remain
explicitly unavailable for exact-subject proof.

### M002 — Eggplan-backed WorkPlan assessment adoption

Status: closed (`plans/closure/eggplan-assessment-integration/003-m002-status.md`; implementations `3e992291` + `3c7438c7`; hosted `36760308368` success).

CodeGG-local implementation plan:

- `plans/implementation/eggplan-assessment-integration/003-staged-production-assessment-adoption.md`

Coordinated Eggplan plan:

- `plans/implementation/codegg-integration/002-staged-eggplan-assessment-adoption.md`

M001 is closed and the staged-adoption bridge is present.

The provenance dependency is satisfied by CodeGG M001 closure
`418fdc85656e7e1faa57f71e5e7f10f7f4859c60`. Eggplan's current pure bridge
remains `088968bd58680ae2b3741e2f1feb0614e0ff81a0`. M002's own verification
digest derivation and differential-adoption requirements remain its scope.

Coordinate with Eggplan
`plans/implementation/codegg-integration/002-staged-eggplan-assessment-adoption.md`.

Use the pure Eggplan CodeGG bridge behind a CodeGG application-layer
assessment facade, derive verification digests from authoritative native
execution specifications, and run differential qualification before migrating
production Git-backed supported-evidence call sites.

The pure `codegg-core::work_plan::assess_work_plan` remains a compatibility
API/differential oracle and explicit legacy engine for non-Git or currently
unsupported evidence kinds; DB/Git authority is not moved into codegg-core.

Implementation may proceed in parallel with the CI timing-flake corrective,
but M002 closure requires a trustworthy green hosted baseline before CI
evidence may be accepted.

Exit condition: CodeGG's existing WorkPlan completion families are produced
through Eggplan's generic assessment for the supported evidence subset with no
permissive regression and no CodeGG runtime/storage ownership transfer.

### M003 — Repository Plan binding

Status: closed. Closure:
`plans/closure/eggplan-assessment-integration/004-m003-status.md`
(implementation `53dea47f`; Eggplan pin `3f7c603`; hosted
`36938461935` success).

CodeGG plan:

- `plans/implementation/eggplan-assessment-integration/004-repository-plan-binding-and-writeback.md`

Eggplan contract plan:

- `eggstack/eggplan:plans/implementation/codegg-integration/003-repository-plan-binding-contract.md`
- planning registration `6ba3db24efb8ed5952be8c5a522a9c4c52f7ed63`

M002 closed positively at `ffa1c15e654776c3ebe1022f4ce7de2582bc5d98`
with exact-head hosted qualification `36760308368`.

M003 binds an explicit existing repository-local Eggplan Plan to a CodeGG
session or one-shot shared-workspace WorkOrder occurrence. Eggplan becomes
canonical for the bound Plan's intent/lifecycle/evidence/closure; CodeGG keeps
a reconciled execution mirror and remains scheduler/session/worktree owner.

The implementation owns v68 binding persistence, independently proves
CodeGG-workspace <-> Eggplan `epr_*` identity before subject translation,
writes repository lifecycle/evidence first, reconciles interrupted mirror
updates, and closes only through Eggplan guarded finalization.

Mutation-capable AutoIsolated WorkOrders are intentionally unsupported because
managed worktrees do not reliably contain the repository-local untracked
`.eggplan` state root. No state copy or silent unbound fallback is allowed.

### C001 — Dirty-subject provenance and bound evidence requalification

Status: ready for coordinated handoff.

CodeGG plan:

- `plans/implementation/eggplan-assessment-integration/005-m003-c001-dirty-subject-provenance-and-bound-evidence.md`

Eggplan corrective:

- `eggstack/eggplan:plans/implementation/codegg-integration/003-c001-dirty-subject-fingerprint-and-bound-evidence-requalification.md`
- planning registration `ee92edc1cf035010fe86ea2382694cc18a0dce45`

Post-closure review found that M003's bound dirty evidence path translated the
CodeGG-native dirty digest into an Eggplan SubjectRevision even though the two
projects intentionally use different digest encodings. The existing dirty test
qualified binding only, not dirty execution -> evidence -> completion ->
guarded closure.

C001 keeps historical M003 closed and repairs only this later defect. Eggplan
will expose its existing digest as a bounded repository-ID-free fingerprint.
CodeGG will persist that exact Eggplan-compatible digest alongside native
attempt provenance, use an E1/C/E2 sandwich for binding-time stability, and
require the persisted digest for bound dirty historical translation. Legacy
dirty provenance remains readable but fails closed for bound exact-subject
evidence; clean and unbound M002 semantics remain unchanged.

## 5. Parallelism

M003 may proceed independently of unrelated tool-advisor, Eggwork M004, and
other closed/corrective workstreams. It is coordinated only with the Eggplan
M003 compatibility-contract implementation.

Historical note — M001 was independent of:

- further Eggwork M003 workspace-transfer optimization; M002/M002a are already
  closed and their current snapshot/policy surface is the M001 baseline;
- tool-selection experiments;
- Eggplan Projection/CLI M002;
- Eggplan Eggwork/Eggsearch adapter M002;
- Eggplan Evidence C003 maintenance.

Do not serialize those unrelated workstreams on this roadmap.

## 6. Verification strategy

M003 qualification adds repository identity proof, v68 binding/restart,
cross-store reconciliation, terminal evidence writeback, guarded repository
closure, and one-shot WorkOrder inheritance to the historical M001/M002
matrix below.

M001/M002 qualification covered:

- clean and dirty Git subjects;
- staged, unstaged, untracked, deleted, symlink, and bounded submodule state;
- attempt-specific retry provenance;
- persistence/restart;
- subject drift;
- remote snapshot manifest digest and completeness;
- symlink/non-regular/oversize omission fail-closed semantics;
- legacy NULL provenance;
- AgentRun linkage to JobAttempt provenance;
- RunStore propagation where a run is emitted;
- no current-worktree historical backfill;
- WorkPlan evidence resolver status/subject separation;
- existing WorkPlan/Goal/Todo/checkpoint/trajectory suites;
- scheduler/job migration and restart suites;
- execution-ownership static guards.

## 7. Completion definition

Historical M003 met the original completion definition. The roadmap is
temporarily active only for post-closure C001; strict current qualification
also requires the dirty-subject corrective to close.

This roadmap closes when CodeGG can use Eggplan as a pure assessment substrate
for WorkPlan evidence without manufacturing historical source identity,
changing execution ownership, or creating a second plan/scheduler persistence
authority.

Met at M003 closure: Eggplan is the canonical Plan/evidence/closure authority
for a bound plan, CodeGG owns a reconciled durable mirror plus all execution
and scheduling authority, historical source identity is proven rather than
manufactured, and no second plan/scheduler persistence authority exists. The
roadmap is closed; M003's §7 evidence is in the M003 closure record.
