# Agent Workstream Blackboard M007 — Per-Workspace Build Admission

Status: closing — per-workspace scheduler admission is implemented independently of M005; the realistic Rust workspace qualification passed through the daemon-backed JobId consumer. Record exact-head CI and reconcile the registry before strict closure.

Planning baseline: 171ef93b6ba67d50b0412ae23b7697eb02d072eb (main, inspected 2026-10-16)
Source: plans/subsystems/agent-workstream-blackboard-roadmap.md M007
ADRs: ADR-0002 and ADR-0001. ADR-0032 is not needed for this isolated-workspace admission scope.
Canonical: plans/000-long-term-specification.md §§17, 26, 28–29; plans/001-terminology-and-domain-model.md §§5, 8.
Primary class: capability and performance infrastructure.

## Objective

Bound concurrent scheduler-owned build jobs per canonical workspace while preserving concurrency across unrelated workspaces. Publish authorized coarse job status to group members. This milestone changes admission only: isolated worktrees keep independent build outputs, Cargo's native locks remain in force, and no shared target directory, build coalescing, compiler cache, or shared checkout is introduced.

## Starting evidence

The existing scheduler and durable job/attempt lifecycle are the authority. A per-workspace quota is scoped by canonical WorkspaceId and does not require shared checkout ownership, M005, or ADR-0032. Any future shared cache or build coalescing is a separate proposal requiring its own compatibility and trust-boundary evidence. CodeGG already submits eligible build/test/lint command routes through JobScheduler and offers background JobId handles and completion notification. `scripts/verify.sh` CARGO_BUILD_JOBS=2 is repository-local, not a per-workstream runtime policy.

## Work packages

A. Typed quota contract. Add a serde-defaulted keyed quota request to ResourceRequest, scoped initially by canonical WorkspaceId and fixed build resource class. Admission acquires/releases weighted slots atomically with existing permit dimensions. Queue caps are not execution quotas. Derive build classification only from supported typed job kinds and canonical inspected routes.

B. Bound concurrency. Define an optional per-workspace concurrent-build limit, fair admission and correct cancellation. Unrelated workspaces remain concurrent. Waiting jobs must not hold a parent execution permit and cause deadlock. No job coalescing is permitted in this milestone.

C. Coarse status. Publish verified scheduler-owned job ID, kind, phase and terminal outcome through existing authorized group projections. Do not expose command arguments, environment values, logs, cache contents or artifact paths. Completion does not automatically start a new model turn.

Build caching, target-directory sharing, deduplication, disk cleanup, and compilation performance studies are out of scope. They require a separate dependency-ready plan after the admission contract is implemented and measured.

## Stop conditions

Stop and retain existing behavior if canonical routing cannot distinguish supported build jobs, if arbitrary shell can bypass scheduler admission, if cancellation leaks quota, or if unrelated workspaces become coupled. A waiting job must not occupy an execution permit. Do not silently classify an unsupported command as a build.

## Tests and comparative evidence

- Two build jobs in one workspace obey a configured running limit; unrelated workspaces can build concurrently.
- Waiting-job cancellation removes only its queue request; active-job cancellation releases its own quota only after its process lifetime ends.
- Kill/restart during build never reports success without a durable successful attempt; terminal failures remain attributable.
- Multiple principals/projects cannot observe private command/log details through group status.
- Unsupported job kinds retain existing admission; non-Rust workspaces do not affect basic build/test operation.
- Verify with deterministic fake executor and one manual realistic development-host Rust workspace fixture; no SBC build/startup requirement and no benchmark CI performance threshold.

Run focused scheduler/job/resource-admission and process-security suites, fmt, scripts/verify.sh quick, Clippy and relevant guards. Record data and limitations in plans/closure/agent-workstream-blackboard/007-status.md; close only after typed per-workspace admission and an authorized production JobId consumer work end to end.

## Deferred cache research (design only; no production change)

This subsection does not authorize shared target placement, build coalescing,
cache reuse, or group shared checkout. M007's per-workspace admission may
proceed independently; any cache strategy requires separate compatibility and
trust-boundary review.

### Current scheduler boundary

- `JobRecord` already carries canonical `workspace_id`; job payloads and
  executor routing remain scheduler-owned.
- `ResourceRequest` persists scalar CPU/memory/process/I/O/network requests
  and string exclusivity keys. `ResourceRequest::for_kind(Build)` currently
  requests the unscoped `exclusive:workspace-mutation` key.
- `PermitDimensions` and `AdmissionController` atomically reserve global
  scalar budgets plus binary exclusivity keys. They do not implement a
  per-key weighted quota. `max_per_workspace` is a queue cap, not a running
  build concurrency limit; `running_per_workspace` is an observation counter.
- `JobSubmissionService` derives default resource policy from typed `JobKind`
  and normalizes legacy key spellings. Model-authored descriptions cannot
  establish build identity or resource class.

### Deferred candidate interface

1. Keep a build's scheduler identity as its durable `JobId`. Reuse the
   existing durable job/attempt lifecycle, cancellation, permits, completion
   and notification path; do not create a build scheduler or cache daemon.
2. Add a typed, serde-defaulted keyed quota request to `ResourceRequest`,
   initially scoped by canonical `WorkspaceId` and a fixed build resource
   class. Admission acquires/relinquishes weighted slots atomically with the
   existing permit dimensions. A config-owned per-workspace slot limit must
   leave unrelated workspaces independent. Queue caps must not be reused as
   execution quotas.
3. Derive build classification only from supported typed job kinds and the
   canonical inspected command route. Unsupported shell/argv routes retain
   existing conservative admission and do not gain compiler-cache reuse.
4. Keep deduplication separate from admission. Any future observer of an
   existing `JobId` requires an exact host-computed fingerprint over argv,
   toolchain, canonical workspace/repository, HEAD and dirty inputs,
   features/target/profile, relevant environment, sandbox and authorization.
   Missing or unmeasurable inputs mean a separate execution. A related job's
   success never proves the caller's current build succeeded.
5. Isolated target directories remain the default. Optional compiler-unit
   cache reuse is a separate opt-in strategy with principal/trust partitioning
   and measurement; never point arbitrary worktrees at one shared Cargo target
   directory. Cargo retains its native locking.
6. Publish only coarse job-owned status through existing authorized job/run
   projections and same-group subscriptions. Never expose command lines,
   environment values, logs, cache contents, or artifact paths through a
   group-wide event.

This deferred cache research is not part of the M007 implementation contract.
No cache strategy has positive performance evidence, and this M007
implementation must not enable one.
