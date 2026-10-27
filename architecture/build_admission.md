# Build Admission — workspace and repository build quotas

M007/A003. This module owns bounded admission for scheduler-owned build
jobs: a required per-workspace quota, and an optional aggregate quota shared
by workspaces in one canonical repository.

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

The default limit remains **one build-class slot per workspace**. That
preserves the old behaviour *within* a workspace — one build-class job
at a time — while letting unrelated workspaces build concurrently. Raising
`scheduler.quota.max_concurrent_builds_per_workspace` widens the bound
deliberately; Cargo's own locking still protects a shared target
directory.

## Where the contract lives

`codegg-core` owns the typed contract (`crates/codegg-core/src/jobs/quota.rs`):

- `ResourceClass` — the closed taxonomy of resource classes. `Build` is
  the required class today, and it covers `JobKind::Build` and
  `JobKind::Format` — precisely the two kinds that used to hold the
  global key. `RepositoryBuild` is an additional namespace accepted only
  for a daemon-resolved repository when the optional cap is configured. The
  taxonomy is closed *in code* so that no caller, and no model, can introduce
  a namespace to obtain capacity.
  `ResourceClass::for_job_kind` is the only mapping from a job kind to a
  class, and it returns `None` for every other kind.
- `KeyedQuotaRequest { key, slots }` — a request to consume weighted
  slots in one scope. The key is `<namespace>:<scope>`
  (`ResourceClass::quota_key`). `KeyedQuotaRequest::unbound(class)` is the
  kind-derived, unscoped request; `KeyedQuotaRequest::for_workspace(class,
  workspace_id)` is the bound one.
- `MAX_QUOTA_KEY_BYTES` (256) and `MAX_QUOTA_SLOTS` (64) — the structural
  bounds a request must satisfy to be checkable at all.

Two enforcement points live outside `codegg-core`:

- `src/scheduler/admission.rs` — `AdmissionController` owns the
  reservation, the limit lookup (`quota_limit`), and the refusal
  decisions.
- `src/scheduler/submission.rs` — `JobSubmissionService` owns workspace
  scope binding and resolves repository scope through the daemon-owned
  `ProjectContextResolver`.

`ResourceRequest` gained a serde-defaulted `keyed_quotas` field, so a
request persisted before M007 decodes unchanged and keeps its historical
admission.

## The limit belongs to the configuration

A job may ask for slots. It may never widen the bound it is measured
against.

`scheduler.quota.max_concurrent_builds_per_workspace` (default `1`) and the
optional `scheduler.quota.max_concurrent_builds_per_repository` are the only
sources of build-class limits. The admission controller
resolves a request's namespace to a limit through its own configuration;
it never reads a limit off the request.

A request naming a namespace the controller does not know is **refused**
(`UnschedulableReason::UnknownQuotaNamespace`), not ignored. An
unenforced bound that was silently dropped would be a lie: the job would
report a reservation it never held.

The controller resolves the limit itself, in
`AdmissionController::quota_limit`: `build` resolves to the workspace cap;
`repository-build` resolves only when the optional repository cap is
configured. Unknown and disabled namespaces resolve to `None`, which the
reservation path refuses.

## A003 — repository-scoped opt-in

The optional `scheduler.quota.max_concurrent_builds_per_repository` adds an
aggregate limit across workspaces that share the same canonical repository.
It is absent by default, preserving existing configuration and behavior.
When set, the submission boundary resolves a `RepositoryId` from the
daemon-owned, resolved workspace binding; a caller path, label, or submitted
resource request cannot choose the scope. Missing, ambiguous, or unresolved
lineage refuses Build/Format submission.

Workspace and repository quotas are checked and reserved atomically under the
admission controller's single short-lived mutex. The admission path aggregates
duplicate keys and checks them in deterministic key order. A blocked job
holds no resource or quota permit. The existing RAII permit is held until the
executor has proved process termination, then releases both dimensions.

Only typed `Build` and `Format` jobs receive repository quota requests. `Test`,
`Lint`, `Shell`, `Python`, `ManagedProcess`, and `ToolProgram` remain outside
this classification; arbitrary Bash is not parsed or represented as a
governed build. This is a deliberate accounting boundary, not a claim that
every external build process is covered. Repository quota status remains a
coarse admission reason and does not expose command, environment, path, or
principal metadata.

## How one request is resolved

`AdmissionController` evaluates each `KeyedQuotaRequest` in a fixed order.
The first step that fires decides the job's fate; nothing is reserved
until every step has passed.

| # | Condition | Decision | Why |
|---|-----------|----------|-----|
| 1 | The request is structurally unusable | `Impossible(UnknownQuotaNamespace)` | A request that cannot be checked is a request that cannot be enforced. |
| 2 | The namespace is not a known class | `Impossible(UnknownQuotaNamespace)` | An unenforced bound, silently dropped, would be a lie. |
| 3 | `slots > limit` | `Impossible(QuotaExceedsLimit)` | The limit is configuration-owned; this is an admin matter, not a transient one. |
| 4 | `slots > available` (the scope is full) | `TemporarilyBlocked(QuotaContended)` | The fair queue retries it later; it is never clamped. |

Step 1 is `KeyedQuotaRequest::is_valid()`: an empty key, a key longer than
`MAX_QUOTA_KEY_BYTES` (256), an empty namespace, or a slot count outside
`1..=MAX_QUOTA_SLOTS` (64) makes the request unusable. The bounds exist so
a durable request cannot grow an unbounded index entry, and so a request
larger than the bound is **refused rather than clamped** — a silently
clamped request would report a reservation the job never held.

Step 2 is the closed-taxonomy guarantee. `ResourceClass::for_job_kind`
maps **only** `JobKind::Build` and `JobKind::Format` to
`ResourceClass::Build`; every other kind returns `None`. Because
`ResourceClass` is a closed enum, no caller — and no model — can mint a
namespace to obtain capacity.

`QuotaExceedsLimit` is the only one of the three refusals that is *not* an
unknown namespace, and it is the one that makes "a job may ask for slots
but never widen the bound" literal: a job asking for two build slots
under a limit of one is refused, not silently reduced to one.

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

## Testing

`tests/scheduler_jobs/build_quota_admission.inc` — eighteen tests, one per
clause of the contract:

- a build quota is scoped to its workspace;
- two builds in one workspace obey the configured limit;
- unrelated workspaces build concurrently;
- a raised limit admits two builds in one workspace;
- a request above the limit is `Impossible`, not merely blocked;
- an unknown quota namespace is refused rather than ignored;
- a malformed quota request is refused;
- a waiting job holds no permit;
- cancelling a waiting job releases only its own queue request;
- a cancelled running job releases its quota when the permit drops;
- a job without a quota is unaffected by the build limit;
- the build class replaces the legacy global exclusivity key;
- only `Build` and `Format` are build class;
- the build-class scope is bound to the canonical workspace;
- an unbound class request still carries a bound;
- the default build limit is one;
- a zero build limit is refused by validation;
- an admitted build consumes its scope and no other.

`tests/scheduler_jobs/scheduler_resource_profiles.inc` covers the
`ResourceRequest::for_kind` resource table the quota rides on.

```bash
cargo test --test scheduler_jobs -- build_quota_admission
cargo test --test scheduler_jobs -- scheduler_resource_profiles
```

`tests/scheduler_jobs/` is a **family**: one executable per subject area,
selected with `--test scheduler_jobs -- <member>`. A bare
`cargo test --test build_quota_admission` selects zero tests and still
exits 0.

## Source verification

Verified 2026-10-23 against source. An earlier pass established this
section; this pass re-checked every claim against the current code and
added the material the document had not yet described.

Confirmed unchanged: `crates/codegg-core/src/jobs/quota.rs` owning the typed
contract, with `ResourceClass` (`:49`) and `KeyedQuotaRequest` (`:97`); the
serde-defaulted `keyed_quotas` field on `ResourceRequest`
(`crates/codegg-core/src/jobs/mod.rs:746`); the configuration-owned limit
`scheduler.quota.max_concurrent_builds_per_workspace`
(`crates/codegg-config/src/schema.rs:2356`, resolved in
`src/scheduler/config.rs:148` with the default `1` at `:180`); the refusal of
an unknown namespace (`UnschedulableReason::UnknownQuotaNamespace`,
`src/scheduler/admission.rs:147`, raised at `:385` and `:393`); the
submission-boundary scope binding and the legacy-key drop
(`src/scheduler/submission.rs:603` `apply_resource_policy`, dropping
`exclusive:workspace-mutation` at `:630`); and the bounded
`AppEvent::JobStatusObserved` payload — `session_id`, `turn_id`, `job_id`,
`kind`, `phase` — with its phase labels `running`, `completed`, `failed`,
`cancelled`, `timed_out`, `interrupted`
(`crates/codegg-core/src/bus/events.rs:420-432`).

Added in this pass, because the source owns them and the document did not
describe them:

- **The four-step request resolution.** The reservation path in
  `src/scheduler/admission.rs` evaluates each request in a fixed order:
  structurally invalid → `Impossible(UnknownQuotaNamespace)`; unknown
  namespace → `Impossible(UnknownQuotaNamespace)`; `slots > limit` →
  `Impossible(QuotaExceedsLimit)`; `slots > available` →
  `TemporarilyBlocked(QuotaContended)`. Nothing is reserved until every
  step has passed.
- **`AdmissionController::quota_limit`** resolves `build` to the workspace
  limit and `repository-build` only when the optional repository limit is
  configured; unknown and disabled namespaces are refused.
- **The structural bounds** `MAX_QUOTA_KEY_BYTES` (256) and
  `MAX_QUOTA_SLOTS` (64) (`crates/codegg-core/src/jobs/quota.rs:35` and
  `:41`), and `KeyedQuotaRequest::is_valid()`, which makes an oversized
  key or an out-of-range slot count a refusal rather than a clamp.
- **`ResourceClass::for_job_kind`** — the only mapping from a job kind to
  a class; it returns `None` for every kind other than `Build` and
  `Format`.
- **The `unbound` / `for_workspace` constructors**, and the `<namespace>:<scope>`
  key shape produced by `ResourceClass::quota_key`.
- **A `## Testing` section**, with the eighteen-test contract in
  `tests/scheduler_jobs/build_quota_admission.inc` enumerated clause by
  clause and the family-correct run commands
  (`cargo test --test scheduler_jobs -- build_quota_admission`).

