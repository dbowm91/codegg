# M005 Writer-Containment Corrective C006 — Closure Status

**Corrective:** C006 — restricted shared-checkout qualification (Q1–Q5).
**Status:** **Conditionally closed.** The shared profile is implemented,
wired and qualified on Linux. Two named gaps keep it from *strict* closure,
and both are recorded as narrow follow-ups in §9.
**Predecessors:** C002 (writer permit), C003 (writer fence), C004 (checkout
reservation and mediation), C005 (process deny contract) — all closed.

---

## 1. Executive finding

The restricted **shared checkout** profile now exists as a real, opt-in
production path. Two bounded agents can be placed on **one** managed
checkout. Their native writes are serialized by the durable writer permit,
so exactly one of them holds the checkout at a time; their reads are not
blocked. Every process-capable surface is refused for the group, by the C005
admission authority and by the child's own tool profile.

Three properties make the profile safe rather than merely functional, and
each is enforced in code rather than in a convention:

1. **The placement is opt-in and never inferred.** A run's durable payload
   carries a `RunPlacement`. The default is `Auto` — the isolated
   worktree the run had before this corrective. Sharing is asked for; it is
   never deduced from a group, a session, or a model request.
2. **The scope is the group's, not the caller's.** The placement names a
   durable group. The group must exist, must still be live, and must list
   the run as a member. A run that is not a member is refused, and the
   refusal is typed. A caller cannot widen the profile by naming a
   checkout it does not belong to.
3. **The profile is platform-gated.** It is available only where there is
   native containment evidence, and the refusal names the platform and the
   missing evidence. It is **off** on macOS and Windows in this tree.

### 1.1 What is live in production today, and what is not

**Live:**

* `RunPlacement` in `codegg-core`, carried by `JobPayload::SubagentRun`.
* `SharedCheckoutCoordinator`, which resolves (and, where it can, creates)
  the group's managed checkout, under the group's own lock.
* `DaemonWriterFenceFactory`, the daemon-side implementation of the tool
  layer's `WriterFenceFactory` seam. This is the piece that makes the
  shared profile **authoritative**: the fence a child runs under is the one
  its own durable run record names.
* The executor's shared branch: a run placed on a group joins that group's
  checkout instead of allocating a second one, and the run's durable record
  is attached to it.
* The task tool's `placement` parameter, with the shared profile narrowing
  the child's effective tool profile.
* The C005 reservation preflight, consulted by the shared placement.

**Not live:**

* **Managed worktree creation.** `git worktree add` needs a contained Git
  backend that does not exist in this tree, so `WorktreeService::create`
  fails with a typed refusal. The tree's own tests assert that refusal
  (`crates/codegg-core/src/worktree_service.rs`,
  `tests/work_orders_m007_trajectory.rs`). This is **pre-existing** and it
  affects the isolated delegated-run path identically — the shared profile
  is not what broke it. The consequence for C006 is precise: the group's
  checkout must **already exist**, and the placement **joins** it. The
  coordinator's create branch is the same code path the isolated path uses,
  and fails the same way.
* **The shared profile on macOS and Windows.** The gate keeps it disabled
  there. See §4.

---

## 2. The mechanism

### 2.1 The placement (`crates/codegg-core/src/run_placement.rs`)

```rust
pub enum RunPlacement {
    #[default]
    Auto,
    IsolatedNew,
    SerializedShared { group_id: AgentRunGroupId },
}
```

`Auto` is the `#[default]`, and the durable payload field is
`#[serde(default)]`. A payload written before the placement existed decodes
as `Auto`, and an older API client keeps exactly the behaviour it had. The
`IsolatedNew` variant exists so that an operator can *pin* the isolated
profile for a run even when the run's group offers a shared checkout; the
distinction is in intent, recorded durably.

### 2.2 The authority (`crates/codegg-core/src/shared_checkout.rs`)

`SharedCheckoutCoordinator` answers one question — *which checkout does this
group share?* — and creates that checkout when the group has none.

* The group's checkout is the managed checkout whose **lease is held by a
  member of the group**. The lease owner is what makes the checkout *this
  group's* checkout, so a checkout leased by an unrelated run is never
  adopted.
* The whole decision is taken under a **per-group lock**, so two members
  that reach it together end up on one checkout.
* The group record is read from the durable group store, never asserted by
  the caller. An unknown group, a finished group and a non-member are three
  distinct, typed refusals.
* The coordinator **fails closed**: an authority that cannot answer is not
  an answer.

The lock is process-local. That is sufficient because the daemon is a
user-scoped singleton, and the durable anchor is the group's member
records, which name the checkout whoever holds it.

### 2.3 The carrier

`JobPayload::SubagentRun` gained one additive, defaulted field:

```rust
#[serde(default)]
placement: crate::run_placement::RunPlacement,
```

The placement travels with the durable job, so a scheduler restart, a retry
and a recovery all see the same placement. No migration was needed: the
payload is tagged JSON.

### 2.4 The production seam (`src/core/writer_fence.rs`)

The tool layer already declared the seam — `WriterFenceFactory` — with the
comment *"The daemon implements this; the agent and scheduler layers only
carry the trait object."* Until now nothing implemented it, so production
wired only the containment-only `WorkspaceWriterFence`.

`DaemonWriterFenceFactory` is that implementation. It resolves the fence
from the run's **own durable record** — never from a model-supplied path —
and returns no fence at all for a run that holds no managed checkout, because
such a run has no lease and therefore nothing to fence.

The fence itself (C003) already had the shared semantics: containment
first, then the lease-and-group authorization check, then the effect-class
gate, then the permit.

### 2.5 The daemon wiring (`src/core/daemon_construct.rs`)

The daemon builds the coordinator and the fence factory and hands both to
the scheduler as one `SubagentAuthority` bundle. The scheduler carries the
trait object and never imports `crate::core`; the authority stays where the
authority belongs.

### 2.6 The child's effective profile (`src/tool/task.rs`)

A shared child is **narrowed before it runs**. The task tool denies every
tool that starts a process in the checkout and every tool that uses a
persistent process the host owns:

```rust
pub fn shared_profile_denied_tools(base: &[String]) -> Vec<String>
```

The list is *derived from the C005 capability table*, so it cannot drift
from the set of tools that can actually reach a process. Two tools are
deliberately **not** denied: `task` (which delegates to a child execution
that runs in its own scope) and `job` (whose executor has its own
admission). Denying the delegating tool would deny the profile to itself —
a shared group is *made of* delegating children.

### 2.7 The C005 reservation preflight

The shared placement calls `ProcessAdmissionService::preflight_reservation`
before the group takes the checkout over. A persistent process that was
started before the checkout became shared keeps the authority it had — it
was launched under a checkout that was not yet shared, and nothing has
stopped it from writing. Rather than pretend such a process is gone, the
placement is refused while one is live in the affected checkout. The
refusal never kills the process: its lifetime belongs to its own owner.

---

## 3. The surface inventory

### 3.1 What the shared profile refuses

| Surface | Refusal | Where |
|---|---|---|
| Native file writes, typed Git mutations | Serialized by the checkout's writer permit | `DaemonWriterFence` |
| `bash`, `terminal`, `python_script`, `test`, `verify`, `ci_watch`, `formatter`, `tool_program` | Denied in the child's profile **and** refused by the C005 admission authority | `shared_profile_denied_tools`, `ProcessAdmissionService` |
| `lsp`, `lsp_read` | Denied in the child's profile — a persistent server was started under an authority the checkout no longer has | `shared_profile_denied_tools` |
| A process effect reaching the fence | Refused with the effect class | `DaemonWriterFence::begin` |
| A run outside the group | Refused with `NotAuthorized` | `DaemonWriterFence::begin` |
| An unknown group, a finished group, a non-member | Refused, typed | `SharedCheckoutCoordinator` |
| A live persistent process in the checkout | Refused, typed | `preflight_reservation` |

### 3.2 What it does not refuse

* **Readers.** A read never takes the permit, so a reader overlaps a writer
  freely. That is the point of a permit rather than a lock.
* **The delegating tools.** `task` and `job` stay available; their children
  and executors have their own scopes.
* **The isolated profile.** A run that did not ask to share is unaffected.
* **External writers.** An editor that writes into the same directory
  outside CodeGG is not mediated. The contract covers CodeGG's own writers,
  and says so.

---

## 4. The platform gate, and the per-OS matrix

The profile is gated by a compile-time table:

```rust
pub const QUALIFIED_SHARED_CHECKOUT_PLATFORMS: &[&str] = &["linux"];
```

A platform with no native containment evidence has the profile **disabled**,
and the refusal names both the qualified platforms and the platform that is
missing the evidence. The gate is deliberately not a runtime flag: an
operator cannot turn the profile on for a platform that has no evidence.

| Platform | Native containment evidence | Profile | Status |
|---|---|---|---|
| Linux | present in this tree | **available** | qualified |
| macOS | absent | **disabled** | refused with a reason; narrow follow-up §9.1 |
| Windows | absent | **disabled** | refused with a reason; narrow follow-up §9.1 |

**What is claimed on Linux, and what is not.** The qualification covers the
*mediation*: the placement, the group authorization, the permit's mutual
exclusion, the C005 preflight, the profile narrowing and the platform gate.
It does **not** cover descendant quiescence: no claim is made on any
platform that a process started before the reservation cannot still be
writing to the checkout. That limit is C005's, and it is not narrowed here.

---

## 5. The open items, named

### 5.1 macOS and Windows native containment evidence

**Status:** open, narrow. The profile is disabled on both platforms by the
gate. The work is to produce the native containment evidence for each
platform and, only then, add it to the qualified table. Until then the
refusal is the correct answer, not a gap in the implementation.

### 5.2 The contained Git backend, and therefore managed worktree creation

**Status:** open, pre-existing, and outside M005's scope.
`git worktree add` needs a contained Git backend that does not exist in this
tree; `WorktreeService::create` therefore fails with a typed refusal, and
the tree's own tests assert it. The shared placement inherits this: a group
must already hold a checkout to be joined. This is recorded because it
bounds what C006 can prove — see §8.

---

## 6. The guard

`scripts/check_shared_checkout.py` is the static backstop. It checks:

1. **The contract's landmarks** — nine files, each with the markers that
   make up the contract: the placement type and its platform gate, the
   coordinator, the durable payload field, the daemon's fence factory, the
   executor's shared branch, the task tool's opt-in parsing, the capability
   table's shared-profile denial list, the worker's use of the resolved
   fence, and the daemon's wiring.
2. **That the profile stays opt-in** — the default placement is still the
   isolated one, the durable field is still `#[serde(default)]`, and the
   executor still attaches the run to the checkout it resolved (the fence
   reads the run's own record, so an unattached run would hold no
   authority).
3. **That the shared profile denies what it must** — the denial list is
   still derived from the capability table and still selects both
   process-reaching shapes, and the task tool still applies it.
4. **That the C005 preflight is still consulted** by the shared placement.

The guard's `--self-test` injects **six** violations — a lost landmark, a
default that became the shared placement, an un-defaulted payload field, a
denial list that stopped selecting the persistent-process tools, a denial
list that stopped being derived from the table, a lost preflight and a lost
executor marker — and proves each is reported. The guard is wired into
`scripts/verify.sh quick` and into the CI `verify` job.

---

## 7. The acceptance matrix

| # | Requirement | Evidence | Result |
|---|---|---|---|
| Q1 | Two mutation-capable agents on one checkout, real SQLite, real Git checkout, real durable group | `tests/shared_checkout_c006.rs` — `a_group_member_joins_the_groups_checkout`, `two_members_racing_for_one_group_end_up_on_one_checkout` | met at the authority level |
| Q2 | One writer at a time; readers overlap; clean release | `two_writers_on_the_shared_checkout_never_overlap` (the second writer is refused with `Held`, and takes the checkout after the first settles) | met |
| Q3 | Every known CodeGG file API and prohibited process surface against the reserved worktree | `a_process_effect_is_refused_on_a_shared_checkout`, `the_shared_profile_denies_every_process_capable_tool`, plus the C005 suite (18 tests) and the C003 suite (10 tests) | met |
| Q4 | Linux, macOS and Windows independently | The gate qualifies Linux and **disables** macOS and Windows with a named reason | met by disabling the unqualified platforms (§9.1) |
| Q5 | A controlled opt-in, a typed error, and an operator-visible reason | `parse_run_placement` in the task tool; the `placement` parameter in the tool's schema; the platform refusal that names the platform | met |
| — | The default is unchanged | `the_default_placement_is_the_isolated_one` — a legacy durable payload decodes as `Auto` | met |
| — | The group cannot grant itself membership | `the_placement_is_refused_for_an_unknown_group`, `the_placement_is_refused_for_a_run_outside_the_group` | met |

---

## 8. Verification evidence

### 8.1 What was run

* `scripts/verify.sh quick` — pass (exit 0).
* `cargo fmt --all -- --check` — pass.
* `cargo clippy --workspace --all-targets -- -D warnings` — pass.
* `tests/shared_checkout_c006.rs` — **15 tests**, 0 failed.
* Regression suites, all green (63 tests in total, 0 failed):
  `process_denial_c005` (18), `checkout_reservation_c004` (14),
  `writer_fence_c003` (10), `writer_disposition_guards` (5),
  `scheduler_executor_coverage` (1), `shared_checkout_c006` (15).
* Guards: `check_shared_checkout.py` (+ `--self-test`),
  `check_process_denial.py` (+ `--self-test`),
  `check_writer_disposition.py` (+ `--self-test`),
  `check_audit_coverage.py` (+ `--self-test`), `check_adr_integrity.py`,
  `check-core-boundary.sh`, `check_daemon_cwd_usage.py` — all pass.

### 8.2 What was NOT run, and why

* **A full-workspace nextest sweep** could not complete on this host:
  another session's `cargo nextest run --workspace --locked --profile ci`
  holds the cargo lock and the load is ~10. The scoped suites above are
  the recorded evidence, and this record says so rather than implying a
  full pass.
* **No hosted CI run.** The branch is local and unpushed, so the new
  guard's CI wiring is asserted by the workflow file, not by a CI result.
* **No macOS or Windows run**, and none is possible here. That is why the
  profile is disabled there.

### 8.3 The composed daemon scenario, and what it does not cover

The plan's Q1/Q2 ask for a composed run through a real daemon, scheduler and
broker with two agents racing for one checkout. This tree cannot compose
that end to end: managed worktree creation needs the contained Git backend
that does not exist here (§1.1), so the delegated-run path that would
allocate a checkout fails before the agent starts — for the isolated path
exactly as for the shared one.

What C006 therefore qualifies is the **mediation**, on a real durable
stack: a real SQLite database with the full migration set, a real durable
agent-run group, a real managed-checkout record over a real Git checkout, a
real durable writer-permit store, the real `SharedCheckoutCoordinator`, the
real `DaemonWriterFence` and the real daemon-side fence factory. The
placement, the group authorization, the mutual exclusion, the preflight and
the profile narrowing are all exercised against those real components.

What it does **not** claim: that two agents have been observed racing for
one checkout through the broker in one daemon process. That scenario is
recorded as a narrow follow-up (§9.2) rather than claimed.

---

## 9. Known limits and non-claims

1. **macOS and Windows are disabled.** The gate keeps the profile off any
   platform without native containment evidence. Adding a platform to
   `QUALIFIED_SHARED_CHECKOUT_PLATFORMS` is a recorded decision with
   evidence behind it, never a quiet edit.
2. **The composed daemon race is not claimed.** See §8.3.
3. **Descendant quiescence is not claimed on any platform.** A process
   started before the reservation can still be writing. C005's preflight
   refuses the *reservation* while such a process is live; it does not stop
   the process.
4. **The shared profile narrows the tool profile, it does not narrow the
   permission.** The permission, sandbox and Git-authorization layers still
   apply and can only narrow further.
5. **The group is the reservation.** A run outside the group cannot write
   the shared checkout, whatever scope it is handed: the run's own durable
   record wins over the scope.
6. **A scheduler-owned child's write is attributed to the daemon.** The
   fence's principal requirement is an *attribution* requirement, and the
   agent loop does not thread the transport principal into the tool batch.
   The daemon therefore names its own local principal, exactly as the
   permission layer already treats a scheduler-owned child. The permit
   still records the individual `agent_run_id`, so the write is
   attributable to the run even when the actor is the daemon. A daemon with
   a team policy must install no fallback; the factory's
   `local_principal_fallback` flag is where that decision lives.

---

## 10. What C006 does and does not unblock

* C006 **does** unblock the next M005 milestone: the shared profile is now
  a real path rather than a capability, and the C002–C005 chain is closed
  end to end.
* C006 **does not** unblock M007's isolated per-workspace scheduling, which
  remains separate.
* The restricted v1 still cannot claim a useful Rust build inside the
  shared checkout: a build is a process, and the shared profile refuses
  every unqualified process. That is the contract, not a defect.

---

## 11. Historical records preserved

* `plans/closure/agent-workstream-m005-writer-containment-corrective/004-status.md`
  — C004, the checkout reservation and the mediation.
* `plans/closure/agent-workstream-m005-writer-containment-corrective/005-status.md`
  — C005, the process deny contract.
* `plans/implementation/agent-workstream-m005-writer-containment-corrective/006-restricted-shared-checkout-qualification.md`
  — the plan this record closes.
