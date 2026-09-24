# Eggplan Assessment Integration Roadmap

Status: active roadmap; M001 requires corrective pass, M002 remains blocked on positive M001

Canonical authority:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`
- `plans/subsystems/long-horizon-work-execution-roadmap.md`

External assessment substrate:

- Eggplan repository: `https://github.com/eggstack/eggplan`
- Eggplan integration blocker head reviewed:
  `a4edb31b90f5f8a8a47e244110aa69747a0ac4bc`
- Eggplan pure CodeGG bridge implementation:
  `088968bd58680ae2b3741e2f1feb0614e0ff81a0`
- Eggplan staged-adoption plan:
  `plans/implementation/codegg-integration/002-staged-eggplan-assessment-adoption.md`

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

Status: corrective pass required; local attempt provenance is implemented, but remote sealing and run/AgentRun correlation remain incomplete.

Plan:

- `plans/implementation/eggplan-assessment-integration/001-durable-execution-subject-provenance.md`

Capture a versioned CodeGG-native source subject at the authoritative execution
input boundary, persist it on JobAttempt, make it available to linked
AgentRun/RunStore evidence resolution, and expose a bounded enriched
host-evidence resolver. Legacy records remain subject-unavailable.

Exit condition: a completed current-generation CodeGG job can resolve its
historical execution subject after restart without consulting the current
worktree, while legacy/missing/drifted cases remain explicitly unavailable.

### M002 — Eggplan-backed WorkPlan assessment adoption

Status: blocked on positive M001 and current Eggplan staged-adoption handoff.

Coordinate with Eggplan
`plans/implementation/codegg-integration/002-staged-eggplan-assessment-adoption.md`.

Use the pure Eggplan CodeGG bridge behind CodeGG's existing assessment surface,
derive verification digests from authoritative native execution
specifications, and run differential qualification before removing duplicated
pure assessment logic.

Exit condition: CodeGG's existing WorkPlan completion families are produced
through Eggplan's generic assessment for the supported evidence subset with no
permissive regression and no CodeGG runtime/storage ownership transfer.

### M003 — Repository Plan binding

Status: deferred; blocked on positive M002.

Allow a CodeGG session/WorkOrder to reference an Eggplan repository Plan and
feed CodeGG job/run/test/artifact observations back through the qualified
adapter contract. Repository identity translation and long-lived Plan binding
belong here, not M001.

## 5. Parallelism

M001 is independent of:

- Eggwork post-closure corrective work except for shared scheduler tests;
- tool-selection experiments;
- Eggplan Projection/CLI M002;
- Eggplan Eggwork/Eggsearch adapter M002;
- Eggplan Evidence C003 maintenance.

Do not serialize those unrelated workstreams on this roadmap.

## 6. Verification strategy

M001/M002 qualification must cover:

- clean and dirty Git subjects;
- staged, unstaged, untracked, deleted, symlink, and bounded submodule state;
- attempt-specific retry provenance;
- persistence/restart;
- subject drift;
- legacy NULL provenance;
- AgentRun linkage to JobAttempt provenance;
- RunStore propagation where a run is emitted;
- no current-worktree historical backfill;
- WorkPlan evidence resolver status/subject separation;
- existing WorkPlan/Goal/Todo/checkpoint/trajectory suites;
- scheduler/job migration and restart suites;
- execution-ownership static guards.

## 7. Completion definition

This roadmap closes when CodeGG can use Eggplan as a pure assessment substrate
for WorkPlan evidence without manufacturing historical source identity,
changing execution ownership, or creating a second plan/scheduler persistence
authority.
