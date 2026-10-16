# Agent Workstream Blackboard M007 — Closure Status

Status: conditionally closed — scheduler admission is implemented and hosted qualification is recorded; the required realistic Rust workspace run remains outstanding

Source implementation plan: `plans/implementation/agent-workstream-blackboard/007-build-resource-coordination.md`
Source subsystem roadmap: `plans/subsystems/agent-workstream-blackboard-roadmap.md#M007--per-workspace-build-admission-active--independent-of-m005`
Implementation commit: `097cba5d`
Exact-head qualification commit: `3ee041db42e2f7cf4be681bd343b527f0881f92d`
Hosted CI: [run 38868243512](https://github.com/dbowm91/codegg/actions/runs/38868243512)

## 1. Executive finding

M007's scheduler-owned per-workspace build-slot admission and authorized
coarse job-status path are implemented independently of M005. Build slot
requests are typed, serde-defaulted and derived from `JobKind::Build` by the
submission boundary. The admission controller accounts slots atomically with
other permit dimensions, scopes them to canonical `WorkspaceId`, and releases
them with the permit after execution lifetime. The default limit is one build
per workspace; unrelated workspaces remain independent. No shared target
directory, compiler cache, build coalescing, or shared checkout was added.

The plan asked for one manual realistic development-host Rust workspace
fixture through an authorized production JobId consumer. The deterministic
scheduler tests and exact-head repository CI do not substitute for that
operational evidence. Keep M007 conditionally closed until that fixture runs
and its result is added here. This condition does not make M007 depend on M005.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Typed, backward-compatible per-workspace quota | `crates/codegg-core/src/jobs/mod.rs` resource request serde/default tests; `src/scheduler/submission.rs` build normalization tests | pass | Build jobs are normalized to one slot; unsupported kinds cannot request build slots through the submission path. |
| Atomic configured admission, same-workspace bound and unrelated-workspace independence | `src/scheduler/admission.rs::build_slots_are_limited_per_workspace_and_release_with_permit`; `src/scheduler/admission.rs::workspace_build_and_mutation_permits_exclude_each_other` | pass | Existing permit lifetime owns release. Workspace identity is required for a slot-bearing request. |
| Authorized coarse job status | existing authorized group job projection and job notification tests; `src/tool/mod.rs` bounded job status adapter | pass in existing suite | Projection remains limited to job identity/kind/phase/outcome. No command, environment, log, cache or artifact-path disclosure was added. |
| Queue/cancel/process-lifetime and restart ownership | scheduler permit/submission/restart-recovery suites and hosted workspace suite | pass in existing deterministic coverage | Queued requests do not own execution permits; active permit release follows executor completion/cancellation cleanup. |
| Production JobId consumer on a realistic Rust workspace | No M007-specific manual run recorded | outstanding | The M008 `CodeGG exec` S0/S1 pilot is a direct agent loop and explicitly did not exercise `JobSubmissionService`. Run a daemon-backed authorized build/test consumer against a realistic Rust workspace. |
| M005 independence and no shared build output | implementation and admission contracts; isolated workspace identity remains canonical | pass | No shared Cargo target or shared-checkout write path was introduced. |

## 3. Implementation and invariant review

The implementation adds a typed workspace build-slot dimension to the
persisted resource request and permit dimensions. `JobSubmissionService`
normalizes the slot count from the typed job kind; callers cannot inflate or
erase the Build quota by supplying custom resource values. The admission
controller holds the workspace slot in the same atomic permit as other
dimensions and releases it when the permit drops. The per-workspace limit is
configurable with a default of one.

Authorized status remains a coarse job projection. M007 does not expose
private execution inputs or outputs and does not start a new model turn on
completion. Cargo retains its native locks and isolated worktrees keep
independent target directories.

## 4. Verification

Implementation coverage includes focused unit tests in
`crates/codegg-core/src/jobs/mod.rs`, `src/scheduler/submission.rs`, and
`src/scheduler/admission.rs`, plus the existing scheduler/job lifecycle and
authorized projection suites.

On the implementation branch, `bash scripts/verify.sh quick`, focused
M006/M007 tests, formatting, `git diff --check`, execution-ownership and
scheduler-bypass guards passed. Exact-head hosted run 38868243512 passed
formatting, all configured repository guards, workspace Clippy and sandbox
helper build. Run 38868243512 failed the count assertion (165 runtime
commands vs. 163 expected); run 38869913826 passed the corrected count and
command-doc assertions but failed the exhaustive built-in action list, which
omitted the two M006 actions. Run 38872186777 passed those command tests, then
revealed two M007 compatibility issues: Build's legacy workspace-mutation
exclusion key was dropped, and the old distinct-Cargo-target parallelism test
did not raise the new default one-build-per-workspace limit. Build now retains
the exclusion key and that test explicitly configures two build slots. The
focused resource-profile, distinct-key contention and build-vs-mutation
admission tests pass locally; a new exact-head hosted run is pending. Do not
use this record as a green hosted-CI claim until it passes.

## 5. Security and recovery review

- Slot identity is the canonical workspace identifier on the durable job.
- Slot acquisition is atomic with other admission dimensions; a queued job
  does not hold the running slot.
- The active permit remains owned for the executor/process lifetime, including
  cancellation cleanup, and releases on completion.
- Only supported typed Build jobs receive this slot class. Unsupported kinds
  and arbitrary shell are not silently relabeled as builds.
- Status visibility uses existing group and project authorization and exposes
  only coarse scheduler-owned facts.
- Shared target placement, build coalescing and cache reuse remain disabled.

## 6. Unresolved finding

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | The requested manual Rust workspace qualification through an authorized daemon-backed production JobId consumer is absent. | Unit and CI coverage establish admission mechanics, but no recorded realistic development-host run proves the end-to-end consumer path and operational behavior. | Run a realistic Rust workspace build/test through the daemon-backed scheduler consumer; record JobId, workspace identity, admission outcome, completion, test result and bounded resource observations. |

## 7. Roadmap disposition and downstream unblocking audit

M007 is conditionally closed. M005 remains active as a separate checkout-wide
writer-containment initiative; M007 does not share checkout or Cargo output.
M006 remains blocked because NewManaged launch lacks a contained Git
materializer. M008 broad qualification therefore remains blocked, now by M006
and its own uncompleted strata/evidence, not by M007's implementation. The
S0/S1 pilot remains descriptive and shows no repeatable latency gain and
higher token use in both repetitions; it does not qualify peer observations
or claims as effectiveness improvements.

No new plan became ready from this conditional closure. When the manual Rust
consumer evidence is supplied, reassess M007's condition and re-audit M008;
M005 shared-write qualification remains separately required for any shared
checkout arm.

## 8. Registry updates

- M007 is listed as conditionally closed, with this record as its status
  authority and the manual Rust workspace run named as the remaining condition.
- M006 remains blocked independently on NewManaged materialization.
- M008 broad comparison remains blocked; the completed isolated S0/S1 pilot is
  recorded separately in `008-pilot-results.md`.
- M005 remains active and shared mutation remains unavailable.
