# Agent Workstream Blackboard M007 — Build-Resource Coordination Across Agent Workspaces

Status: blocked — production changes require M005 closure; bounded interface research is recorded below

Planning baseline: de49884f3fdc6d528620f2a2645694b9b19fc087 (2026-10-15)
Source: plans/subsystems/agent-workstream-blackboard-roadmap.md M007
ADRs: ADR-0002 and ADR-0001; accepted ADR-0032 required for shared group checkout.
Canonical: plans/000-long-term-specification.md §§17, 26, 28–29; plans/001-terminology-and-domain-model.md §§5, 8.
Primary class: capability and performance infrastructure.

## Objective

Reduce redundant compiler work and runaway CPU/disk amplification when agents act in multiple Git worktrees or share a serialized workstream. Treat compilation as heavy scheduler-owned work with explicit cross-agent job-status visibility. Preserve fast ordinary build/test behavior in a single-user session and never require a separate compiler daemon.

## Starting evidence

CodeGG already submits eligible build/test/lint command routes through JobScheduler and offers background JobId handles and completion notification. src/scheduler/executors.rs gives subagents distinct workspace roots. Cargo defaults to checkout-local target/, so separate worktrees may independently compile the same graph. scripts/verify.sh CARGO_BUILD_JOBS=2 is repository-local, not a per-workstream runtime policy. CARGO_HOME generally reuses downloads, not build targets. Optional sccache and CARGO_TARGET_DIR behavior must be validated, not assumed.

## Work packages

A. Measurement and resource model. Instrument JobScheduler resource requests with workstream/workspace/repository/build-family keys and record spawned-build CPU time, wall time, peak RSS (where supported), target/cache disk deltas, queue time, cache compatibility and failure/retry state. Redact sensitive command arguments; never let model-authored 'this is a build' replace canonical command routing/inspection. Avoid permanent high-cardinality metrics.

B. Bound concurrency. Define optional per-workspace concurrent-build limit, no global forced serialization across unrelated repositories, fair admission and correct cancellation. Reuse existing scheduler resource/exclusivity keys. Ensure a waiting task doesn't hold its parent execution permit and cause deadlock. An identical already-running build may return a safe observer/subscription to its JobId only if command identity, toolchain, worktree HEAD/dirty inputs, environment, features, target, profile and sandbox/authorization are compatible; otherwise queue separate executions. Coalescing must not mistake stale outputs for current work.

C. Rust adapter, opt-in. Detect Cargo workspace manifest/root and relevant target/profile/toolchain/feature/RUSTFLAGS/environment fingerprint. Make an explicit strategy choice: isolated target (safe default), reuse compatible target with Cargo's locking cost stated, or optional sccache for compatible compiler units. Preserve Cargo's own locking and external process safety. Do not silently set one CARGO_TARGET_DIR across arbitrary different worktrees/revisions; do not share target containing untrusted outputs across principals without trusted cache separation. No assumptions that sccache is installed.

D. Group blackboard. Publish coarse verified build status (job ID, kind, phase, success, compatible dependency key, artifacts/validation handles, owner) to group members. A peer waits on/subscribes to a scheduler-owned job rather than writing ad-hoc shell polling loops. A completion event does not automatically create a new model turn unless ADR-0031 continuation grant exists.

E. Cleanup. Retention/disk pressure surfaces report checkout and build output footprint separately. Build cache cleanup follows bounded policy, skips active jobs and never calls indiscriminate cargo clean on other users' worktrees. Keep no new permanent cross-process cache service.

## Stop conditions

Stop and retain existing build/test behavior if the scheduler cannot prove source/environment compatibility for deduplication, if cache reuse would cross principal or sandbox trust boundaries, if an arbitrary shell build can evade resource admission, or if target sharing amplifies disk/CPU without a positive measured outcome. Do not silently mark a build successful because a related job completed. File a focused corrective with negative evidence instead of adding global environment changes.

## Tests and comparative evidence

- Two isolated Rust worktrees with equivalent dependency graph; distinguish download cache, compiler cache, incremental artifacts and Cargo target reuse. Collect measured CPU time, wall time, peak disk, job contention for strategies.
- Same HEAD but different features or RUSTFLAGS cannot reuse incompatible outputs; dirty states and branch drift invalidate coalescing correctly.
- Two build jobs in same shared workstream serialize appropriately; unrelated repos can build concurrently; wait cancellation releases only its own observer/permit.
- Kill/restart during build: no duplicate side effect or claimed success, no stale cached success on new HEAD; failed/timeout job status remains attributable.
- Multiple principals and projects cannot read private command/log/cache artifacts or poison shared compiler outputs.
- Missing Cargo, missing sccache, non-Rust repository and unsupported OS measurement degrade cleanly without blocking basic build/test operation.
- Verify with deterministic fake executor and one manual realistic development-host Rust workspace fixture; no SBC build/startup requirement and no benchmark CI performance threshold.

Run focused scheduler/test/background-job/resource-admission and process-security suites, fmt, scripts/verify.sh quick, Clippy and relevant guards. No global hardcoded Cargo target policy. Record data and limitations in plans/closure/agent-workstream-blackboard/007-status.md; close only after production JobId consumer and opt-in resource policy work end to end.

## M007 interface research disposition (design-only; no production change)

This subsection is eligible before M005 under the roadmap's explicit interface
exception. It does not unblock production resource policy, shared target
placement, build coalescing, cache reuse, or group shared checkout.

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

### Candidate interface for a later implementation plan

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

This is an interface candidate, not a contract: the M005 writer boundary,
accepted ADR-0032, scheduler owner review, and resource-admission tests must
confirm or replace it before implementation. No cache strategy has positive
performance evidence yet.
