# M007 — Per-Workspace Build Admission — closure record

**Status:** closed
**Milestone:** M007 — Per-Workspace Build Admission
**Plan:** `plans/implementation/agent-workstream-blackboard/007-build-resource-coordination.md`
**Roadmap:** `plans/subsystems/agent-workstream-blackboard-roadmap.md` (M007)
**Closed:** 2026-10-19
**Branch:** `work/agent-workstream-m005-writer-audit`

## 1. What this milestone is

M007 bounds concurrent scheduler-owned build jobs **per canonical
workspace**, while leaving unrelated workspaces independent, and
publishes a **coarse** job status to authorized group members.

It is an **admission** milestone. It does not add a shared target
directory, a build cache, build coalescing, or a shared checkout, and it
does not enable shared mutation.

## 2. The defect it fixes

`ResourceRequest::for_kind` gave `Build` and `Format` the exclusivity key
`exclusive:workspace-mutation`. An exclusivity key is a binary mutex over
a name, and the name was **unscoped** — it did not say *which* workspace.

The roadmap already recorded the key as a defect:

> Its serialized contention key is the literal global
> `exclusive:workspace-mutation`, rather than scoped to a canonical
> workspace.
> — `plans/subsystems/agent-workstream-blackboard-roadmap.md`

So a build in one workspace excluded a build in **every** other
workspace, and the bound was fixed at one rather than configurable.

M007 replaces it with a counted, workspace-scoped quota whose limit is
configuration-owned.

## 3. What changed

### 3.1 The typed contract (`codegg-core`)

New module `crates/codegg-core/src/jobs/quota.rs`:

- `ResourceClass` — the closed taxonomy of resource classes. `Build` is
  the only class, and it covers `JobKind::Build` and `JobKind::Format`:
  precisely the two kinds that used to hold the global key. The
  taxonomy is closed **in code**, so no caller and no model can introduce
  a namespace to obtain capacity.
- `KeyedQuotaRequest { key, slots }` — a weighted, keyed slot request.
  The key is `<namespace>:<scope>`; the scope is host-computed.
- `MAX_QUOTA_KEY_BYTES` (256), `MAX_QUOTA_SLOTS` (64) — the bounds.
  A request that cannot be checked is a request that cannot be enforced.

`ResourceRequest` gained `keyed_quotas: Vec<KeyedQuotaRequest>`, marked
`#[serde(default, skip_serializing_if = "Vec::is_empty")]`. A request
persisted before M007 decodes unchanged and keeps its historical
admission.

`ResourceRequest::for_kind` no longer emits `exclusive:workspace-mutation`
for `Build` or `Format`, and instead attaches the **unbound** build
class. The scope is bound later, where the workspace is known.

### 3.2 The admission controller

`src/scheduler/admission.rs`:

- `InnerState.held_quotas: HashMap<String, u32>` — weighted slots held
  per scope.
- `BlockReason::QuotaContended { key, requested, available }` — the
  scope is named, so a diagnostic can say *which* workspace is at its
  bound.
- `UnschedulableReason::QuotaExceedsLimit { key, requested, limit }` —
  the request wants more slots than the scope's limit. A configuration
  matter, not a transient one.
- `UnschedulableReason::UnknownQuotaNamespace { namespace }` — the
  request names a namespace the controller does not know. **Refused, not
  ignored**: an unenforced bound that was silently dropped would be a
  lie.
- `AdmissionController::quota_limit` — resolves a namespace to its
  configuration-owned limit.
- Quota checks run **before** any dimension is reserved, and the
  reservation is a single step afterwards. A blocked job therefore
  consumes no process slot, no CPU weight and no memory.
- Quota slots are released when the permit guard drops, which is after
  the executor's process lifetime ends.
- `AdmissionSnapshot.held_quotas` — the held scopes are observable.

### 3.3 The configuration

- `src/scheduler/config.rs`: `QuotaPolicy {
  max_concurrent_builds_per_workspace }`, default **1**. Validated: a
  zero limit is refused (`SchedulerConfigError::ZeroBuildQuota`).
- `crates/codegg-config/src/schema.rs`: `SchedulerQuotaConfig` and
  `SchedulerConfig.quota`, with a `merge` arm.

The default of one preserves the old behaviour *within* a workspace
while letting unrelated workspaces build concurrently.

### 3.4 The submission boundary

`src/scheduler/submission.rs::apply_resource_policy` binds the class to
the job's canonical `WorkspaceId` and drops the legacy global key. The
binding is applied **unconditionally** — not only for a defaulted
request — so a caller that supplied its own resource request cannot opt
out of the bound it is measured against.

A request that never reached the boundary keeps the namespace with an
empty scope (`build:`): one shared global bucket. It is still counted,
and counted against one bucket, rather than escaping the bound.

### 3.5 The coarse group status

- New `AppEvent::JobStatusObserved { session_id, turn_id, job_id, kind,
  phase }` in `crates/codegg-core/src/bus/events.rs`, with a `kind()`
  arm. It carries **no** command line, environment, logs, cache contents
  or artifact path.
- The scheduler publishes it at the running and terminal transitions
  (`src/scheduler/scheduler.rs`), where it owns the verified job record.
  The terminal event is published only **after** the terminal state is
  durably accepted, so it describes a real transition.
- `src/core/mod.rs`: the event maps to `None` on the `CoreEvent` bridge —
  it is an in-process same-group subscription, not a client protocol
  event.
- `src/agent/follow_up.rs`: the consumer's predicate is the same
  authorization the other peer hints use — the event must be scoped to
  the observer's session, the observer must be a member of a group in it,
  and the job must not be the observer's own.

### 3.6 The job projection

- `CoreEvent::JobStarted` / `JobCompleted` / `JobFailed` now carry
  `session_id`, `workspace_id` and `kind`. The fields are additive and
  defaulted, so an event published before M007 still decodes, and a
  newer event still decodes against an older reader that ignores unknown
  fields.
- `crates/codegg-protocol/src/projection/adapters.rs`: the adapters
  carry the facts into `JobProjection` instead of writing empty
  strings.
- `crates/codegg-protocol/src/projection/snapshot.rs`:
  `SessionProjectionSnapshot::upsert_job` now **merges**. A lifecycle
  event that knows only the job id and the attempt id no longer erases
  the kind and the workspace the creation event recorded. This was a
  pre-existing data-loss defect: the board's job fact could never
  resolve.

### 3.7 Documentation

- New `architecture/build_admission.md`.
- `architecture/overview.md`: the module-map row, the deep-dive index,
  and the re-measured counts.

## 4. Requirement-to-evidence matrix

| Requirement | Evidence |
|---|---|
| Per-workspace build bound | `tests/build_quota_admission.rs::two_builds_in_one_workspace_obey_the_configured_limit` |
| Unrelated workspaces stay concurrent | `…::unrelated_workspaces_build_concurrently` |
| The limit is configuration-owned | `…::the_default_build_limit_is_one`, `…::a_zero_build_limit_is_refused_by_validation` |
| A request may not widen its bound | `…::a_request_above_the_limit_is_impossible_not_merely_blocked` |
| An unknown namespace is refused, not ignored | `…::an_unknown_quota_namespace_is_refused_rather_than_ignored` |
| A malformed request is refused | `…::a_malformed_quota_request_is_refused` |
| A waiting job holds no permit | `…::a_waiting_job_holds_no_permit` |
| Waiting-job cancellation touches only its own request | `…::cancelling_a_waiting_job_releases_only_its_own_queue_request` |
| An active job's quota is released only after its process lifetime | `…::a_cancelled_running_job_releases_its_quota_when_the_permit_drops` |
| Only supported typed kinds are build-class | `…::only_build_and_format_are_build_class` |
| The scope is the canonical workspace | `…::the_build_class_scope_is_bound_to_the_canonical_workspace` |
| The scope is bound at the submission boundary | `…::the_submission_boundary_binds_the_build_class_scope` |
| The legacy global key is gone | `…::the_build_class_replaces_the_legacy_global_exclusivity_key` |
| An unsupported kind keeps its existing admission | `…::a_job_without_a_quota_is_unaffected_by_the_build_limit` |
| The group status carries no private detail | `architecture/build_admission.md` §"The coarse status a group may see"; the event's field list |
| The projection keeps the job's facts | `crates/codegg-protocol/src/projection/snapshot.rs::upsert_job` |

## 5. Tests

`tests/build_quota_admission.rs` — 18 tests, all against a real admission
controller. The submission-boundary case is additionally covered by three
unit tests in `src/scheduler/submission.rs` that call the boundary
directly, including the case where a caller supplies its own resource
request and tries to escape the bound.

## 6. Verification

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | pass |
| `cargo test --test build_quota_admission` | 18 passed |
| `cargo test -p codegg-core --test session_launch` | 11 passed (M006, shared run) |
| `cargo test -p codegg-core --lib jobs::quota` | 5 passed |
| `cargo test -p codegg-core --lib jobs` | 38 passed |
| `cargo test --test scheduler_resource_profiles` | 8 passed |
| `cargo test --test scheduler_permit_lifecycle` | 19 passed |
| `cargo test -p codegg-core --lib --tests` | whole crate green |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `scripts/verify.sh quick` | pass |

### A pre-existing failure this milestone surfaced

`crates/codegg-core/tests/user_prompt_queue.rs` and
`…/user_prompt_queue_mutation.rs` both **hard-coded** the storage layout
version (`assert_eq!(STORAGE_LAYOUT_VERSION, 83)`). C002 bumped the
layout to 84 and did not update them, so both tests had been failing
since `7fb56e11`. Nothing caught it: `verify.sh quick` does not run the
workspace tests, and the failure was invisible to every gate CI runs.

M006's own v85 bump (now v87) would have failed them again. Both assertions are
now **floors** (`>= 80` and `>= 82`) rather than equalities. The
relationship that actually matters — the constant equals the highest
wired migration — is already asserted directly above in both files, and
it fails closed if a bump ever lands on one side only. The equalities
tested nothing that the floors do not, and broke on every unrelated
migration.

### What was **not** run

The **full workspace test suite** (`cargo nextest run --workspace --locked
--profile ci`) was **not** completed on this host: the run stalled for
more than twenty minutes on a cargo lock held by another session on the
same machine, and was cancelled. The suite was never observed to fail —
it never got past the lock — but it was not observed to pass either.

The verification above is therefore: the whole workspace **compiles**
(`cargo check --workspace --all-targets`, inside `verify.sh quick`), the
whole workspace is **clippy-clean** at `-D warnings`, every guard in
`verify.sh quick` passes, the whole `codegg-core` crate's tests are
green, and the root-crate test suites that cover the changed areas pass.
What is missing is the broad regression sweep over the ~6,700 root-crate
tests.

The one known pre-existing failure in that sweep is unchanged by this
milestone: `check_policy_surface.py` fails on `tool_batch.rs` ("Always-choice
persist must derive the capability scope"). That guard is in neither
`verify.sh quick` nor CI, and it fails identically on the base branch.

The three `agent_loop_harness` failures recorded earlier in this line are
**environmental**, not code: this machine's `~/.cache` is mode 777 and
`src/scratch.rs:474` refuses a group/world-writable cache root. Pointing
`XDG_CACHE_HOME` at a 700-mode temporary directory makes that suite pass
67/67.

## 7. Known gaps and deferred work

These are **deliberately** out of M007's scope, and are recorded here so
that they are not mistaken for omissions:

- **No shared target directory, build cache, or build coalescing.** The
  plan defers them; M007 changed admission only.
- **No compiler-cache reuse.** Unsupported shell/argv routes keep their
  existing conservative admission and do not gain one.
- **`cargo` keeps its native locking.** Two builds in one workspace can
  run concurrently only when the operator raises the limit, and Cargo's
  own lock then protects the shared target directory.
- **The same-group status is a hint.** The durable job record remains
  the authority for a job's kind and terminal state; the event never
  carries the command line, environment, logs, cache contents or
  artifact paths.
- **The `task action=board` consumer is the production JobId consumer**
  for the observer's own workspace. A peer member's job in another
  workspace is not observable through the board: the board's join is
  workspace-scoped, and widening it is an M005 authorization question,
  not an M007 one.

## 8. What this milestone did **not** do

- It did not enable shared mutation, a shared checkout, or the M005
  writer containment. Those remain blocked behind C003–C006.
- It did not classify an arbitrary shell command as a build.
- It did not start a model turn when a build finished.
- It did not touch the durable writer permit, the checkout lease, or the
  scheduler's exclusivity keys beyond the one global key it replaced.
