# Build Admission — per-workspace build quotas

M007. This module owns one thing: **how many scheduler-owned build
jobs may run in one canonical workspace at a time**, and the coarse job
status a group is allowed to see about them.

It is an *admission* milestone. It does not add a shared target
directory, a build cache, build coalescing, or a shared checkout, and it
does not enable shared mutation.

## The bound is a counted quota, not a mutex

The scheduler already had a bound on build-class work, and it was the
wrong shape.

`ResourceRequest::for_kind` gave `Build` and `Format` the exclusivity
key `exclusive:workspace-mutation`. An exclusivity key is a **binary
mutex over a name**: at most one holder, ever. The name was also
**unscoped** — it did not say *which* workspace. The result was that a
build in one workspace excluded a build in every other workspace, and
the roadmap recorded the key as a defect:

> Its serialized contention key is the literal global
> `exclusive:workspace-mutation`, rather than scoped to a canonical
> workspace.

M007 replaces it with a **counted, scoped quota**:

| | before | after |
|---|---|---|
| shape | binary mutex | weighted slot count |
| scope | global | canonical `WorkspaceId` |
| limit | fixed at one | configuration-owned |
| unrelated workspaces | serialized | independent |

The default limit is **one build-class slot per workspace**. That
preserves the old behaviour *within* a workspace — one build-class job
at a time — while letting unrelated workspaces build concurrently. Raising
`scheduler.quota.max_concurrent_builds_per_workspace` widens the bound
deliberately; Cargo's own locking still protects a shared target
directory.

## Where the contract lives

`codegg-core` owns the typed contract (`crates/codegg-core/src/jobs/quota.rs`):

- `ResourceClass` — the closed taxonomy of resource classes. `Build` is
  the only class today, and it covers `JobKind::Build` and
  `JobKind::Format` — precisely the two kinds that used to hold the
  global key. The taxonomy is closed *in code* so that no caller, and no
  model, can introduce a namespace to obtain capacity.
- `KeyedQuotaRequest { key, slots }` — a request to consume weighted
  slots in one scope. The key is `<namespace>:<scope>`.

`ResourceRequest` gained a serde-defaulted `keyed_quotas` field, so a
request persisted before M007 decodes unchanged and keeps its historical
admission.

## The limit belongs to the configuration

A job may ask for slots. It may never widen the bound it is measured
against.

`scheduler.quota.max_concurrent_builds_per_workspace` (default `1`) is
the only source of the build-class limit. The admission controller
resolves a request's namespace to a limit through its own configuration;
it never reads a limit off the request.

A request naming a namespace the controller does not know is **refused**
(`UnschedulableReason::UnknownQuotaNamespace`), not ignored. An
unenforced bound that was silently dropped would be a lie: the job would
report a reservation it never held.

## The scope is bound at the submission boundary

`ResourceRequest::for_kind` names the *class* but cannot scope it: the
canonical workspace is only known where the job is submitted.

`JobSubmissionService::apply_resource_policy` is that boundary. It binds
the class to the job's `WorkspaceId`, and it does so **unconditionally** —
not only for a defaulted request — so a caller that supplied its own
resource request cannot opt out of the bound it is measured against. The
same pass drops the legacy `exclusive:workspace-mutation` key, because
leaving it in place would keep the global coupling this milestone
removes.

A request that never reached the boundary keeps the namespace with an
empty scope (`build:`). That is a single shared global bucket: it is
still counted, and counted against one bucket, rather than escaping the
bound.

## A waiting job holds no permit

The plan forbids a waiting job from holding a parent execution permit,
because that is a deadlock. The quota respects it:

- A blocked job is refused *before* any dimension is reserved. The
  reservation happens in one step, after every check has passed, so a
  blocked attempt consumes no process slot, no CPU weight and no memory.
- The quota slots are released when the **permit guard drops**, which is
  after the executor's process lifetime ends. A cancelled job never
  returns its slots early.

The queue caps are not execution quotas. `max_per_workspace` bounds how
many jobs may *wait* in a lane; the quota bounds how many may *run*.

## The coarse status a group may see

The group-visible status is deliberately narrow. `AppEvent::JobStatusObserved`
carries:

- the job id,
- the typed job kind,
- a bounded phase label (`running`, `completed`, `failed`, `cancelled`,
  `timed_out`, `interrupted`),
- the session and turn it belongs to.

It never carries the command line, the environment, the logs, the cache
contents or an artifact path. The durable job record is the authority
for those; this event is a hint, not a record.

The event is published by the scheduler, which owns the verified job
record, and is consumed by a same-group agent loop. The consumer's
predicate is the same authorization the other peer hints use: the event
must be scoped to the observer's session, the observer must be a member
of a group in it, and the job must not be the observer's own.

The job projection keeps the same facts. `CoreEvent::JobStarted`,
`JobCompleted` and `JobFailed` now carry the session, the workspace and
the kind, and `SessionProjectionSnapshot::upsert_job` **merges** rather
than replaces: a lifecycle event that knows only the job id and the
attempt id no longer erases the kind and the workspace the creation
event recorded.

## What this module does not do

- It does not classify an arbitrary shell command as a build. The
  classification comes from the typed job kind, which itself comes from
  a canonical inspection of the command route.
- It does not coalesce builds, share a target directory, or add a
  compiler cache.
- It does not enable a shared checkout.
- It does not start a model turn when a job completes.
