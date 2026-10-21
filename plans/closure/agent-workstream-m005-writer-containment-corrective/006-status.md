# M005 Writer-Containment Corrective C006 — Closure Status

**Corrective:** C006 — restricted shared-checkout qualification (Q1–Q5).
**Status:** **Conditionally closed.** The shared profile is implemented,
wired and qualified on Linux and macOS. Two named gaps keep it from *strict*
closure, and both are recorded as narrow follow-ups in §9.
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
   missing evidence. It is **on** for Linux and macOS — each qualified on a
   native runner — and **off** on Windows, which has no filesystem-enforcement
   backend registered at all.

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
* **The shared profile on Windows.** The gate keeps it disabled there, for a
  named reason: there is no filesystem-enforcement backend registered for
  Windows at all. macOS is qualified — see §4 and §5.1.

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
pub const QUALIFIED_SHARED_CHECKOUT_PLATFORMS: &[&str] = &["linux", "macos"];
```

A platform with no native containment evidence has the profile **disabled**,
and the refusal names both the qualified platforms and the platform that is
missing the evidence. The gate is deliberately not a runtime flag: an
operator cannot turn the profile on for a platform that has no evidence.

| Platform | Native containment evidence | Profile | Status |
|---|---|---|---|
| Linux | Landlock, qualified in this tree | **available** | qualified |
| macOS | Seatbelt, qualified on a native macOS runner (lane run `39616896281`, 2026-10-21) | **available** | qualified |
| Windows | **no filesystem-enforcement backend is registered at all** | **disabled** | refused with a reason; the gap is a capability, not an evidence gap — §5.1 |

The macOS qualification is `tests/macos_native_shared_checkout_containment.rs`
on a native macOS runner: the backend is obtained, it is Seatbelt, and a
process launched through the real `ManagedProcessService` under a real
`SandboxRequest::Required` can write inside its own roots and **cannot**
write into a checkout outside them. The fixture *fails* a host that cannot
obtain Seatbelt rather than skipping it, so a skipped qualification can never
be read as a passed one.

**What is claimed on Linux and macOS, and what is not.** The qualification covers the
*mediation*: the placement, the group authorization, the permit's mutual
exclusion, the C005 preflight, the profile narrowing and the platform gate.
It does **not** cover descendant quiescence: no claim is made on any
platform that a process started before the reservation cannot still be
writing to the checkout. That limit is C005's, and it is not narrowed here.

---

## 5. The open items, named

### 5.1 macOS and Windows native containment evidence

**Status:** **macOS closed; Windows open and unreachable.**

**macOS — closed.** macOS's native backend is Seatbelt's, it *is* a registered
backend in this tree, and it *is* applied by the shipped helper. What was
missing was a lane that ever runs it: `tests/sandbox_containment.rs` carries
Seatbelt arms that had never been executed on a macOS host. Two things were
added on 2026-10-21:

* `tests/macos_native_shared_checkout_containment.rs` — a whole-file
  `#![cfg(target_os = "macos")]` fixture that qualifies the dimension the
  shared profile actually needs: a process launched through the real
  `ManagedProcessService` under a real `SandboxRequest::Required` can write
  inside its own roots and **cannot** write into a checkout outside them. A
  macOS host that cannot obtain Seatbelt **fails** the fixture rather than
  skipping it, because a skipped qualification is indistinguishable from a
  passed one in a test report.
* `.github/workflows/shared-checkout-macos-qualification.yml` — a hosted lane
  that runs that fixture, the C006 suite and the containment matrix on a
  native macOS runner, and uploads the logs as an artifact.

The lane run **`39616896281`** (2026-10-21, head `ec1816ad8`) passed: the
native containment fixture 2/2, the C006 suite 16/16, and the containment
matrix 16/16 on a native macOS runner. `macos` was therefore added to
`QUALIFIED_SHARED_CHECKOUT_PLATFORMS` — a recorded decision with that
evidence behind it, not a quiet edit.

The lane was then **re-run on the flip itself** — run **`39627440691`**
(2026-10-21, head `2cddbff3c`, the commit that adds `macos` to the
table) — and it passed again: the native containment fixture 2/2, the C006
suite 16/16, and the containment matrix 16/16. That second run is the
evidence for the *changed* behaviour: `the_shared_profile_is_platform_gated`
now asserts **available** on macOS instead of asserting the refusal, so a
macOS host is proved to take the shared profile rather than merely to
contain a process. The Seatbelt arms reported the real backend —
`Enforced { backend: BackendId("seatbelt") }`, with the guarantee
"writes confined to the workspace and scratch space" — so the lane
observed enforcement, not merely a registered backend.

**Windows — a capability gap, not an evidence gap.** No filesystem-enforcement
backend is registered for Windows at all, so a constrained request is refused
before any process is created. No CI run can produce the missing evidence,
because the capability is absent rather than unmeasured: the Windows half of
the sibling native-qualification corrective is closed *as blocked* for the
same reason. The profile therefore stays disabled on Windows, and the refusal
names a missing capability rather than a missing measurement.

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
| Q4 | Linux, macOS and Windows independently | The gate qualifies Linux and macOS on native containment evidence, and **disables** Windows with a named reason — it has no filesystem-enforcement backend registered at all, so no run could produce the missing evidence | met by disabling the unqualified platform (§9.1) |
| Q5 | A controlled opt-in, a typed error, and an operator-visible reason | `parse_run_placement` in the task tool; the `placement` parameter in the tool's schema; the platform refusal that names the platform | met |
| — | The default is unchanged | `the_default_placement_is_the_isolated_one` — a legacy durable payload decodes as `Auto` | met |
| — | The group cannot grant itself membership | `the_placement_is_refused_for_an_unknown_group`, `the_placement_is_refused_for_a_run_outside_the_group` | met |

---

## 8. Verification evidence

### 8.1 What was run

* **Hosted CI** — `CI / verify` run **`39601564349`** on head `717cdcbc9`
  (`workflow_dispatch` on `work/agent-workstream-m005-writer-audit`). Every
  guard step passed, including the C006 shared-checkout guard and its
  self-test, plus formatting and workspace clippy. The workspace test step
  ran **4,449 tests: 4,448 passed, 1 failed**. The single failure was
  `tool_advisor::causal_frontier::tests::m003_checked_in_receipt_matches_recomputation`,
  and it was a **real regression this branch had introduced** — see §8.4.
  (nextest stops at the first failure by default, so that run never reached
  the integration test binaries; the second run below did.)
* **Hosted CI (second)** — `CI / verify` run **`39616654598`** on head
  `ec1816ad8`. The receipt test now passes and the run reached the
  integration binaries: **9,782 tests run, 9,781 passed, 1 failed**, the
  failure being `scheduler_contention::different_exclusivity_keys_run_in_parallel`,
  which is **pre-existing on this branch and not C006's** — see §8.5.
* **Hosted Windows** — `Windows native probe contract` run **`39616650957`**
  on head `ec1816ad8`: **success**. The branch now compiles and its native
  probe matrix passes on a Windows runner.
* **Hosted macOS** — `Shared checkout macOS qualification` run
  **`39616896281`** on head `ec1816ad8`: **success**. The native containment
  fixture 2/2, the C006 suite 16/16, and the containment matrix 16/16 on a
  native macOS runner. See §5.1.
* **Hosted macOS (on the flip)** — `Shared checkout macOS qualification`
  run **`39627440691`** on head `2cddbff3c`, the commit that adds `macos`
  to the qualified table: **success**. The C006 suite 16/16 with
  `the_shared_profile_is_platform_gated` now asserting *available* on
  macOS, the native fixture 2/2, and the containment matrix 16/16 with
  Seatbelt enforcement reported. See §5.1.
* **Hosted macOS (on the merged head)** — `Shared checkout macOS
  qualification` run **`39635541949`** on head `4710b2966` (the merge
  reconciliation): **success**. The C006 suite 16/16 with the gate
  asserting *available* on macOS, the native fixture 2/2, and the
  containment matrix 16/16 with Seatbelt enforcement reported — so the
  merge did not disturb the qualification.
* **Hosted Windows (on the merged head)** — `Windows native probe contract`
  run **`39635542007`** on head `4710b2966`: **success**. The AppContainer
  native probe matrix 1/1 and the restricted-child
  timeout/cancellation/descendant coverage 6/6 on a Windows runner — so the
  merge did not disturb the Windows compile fix either.
* **Local, on the merged tree** — `scripts/verify.sh quick` — pass (exit 0);
  `cargo check --workspace --all-targets --locked` — pass (exit 0);
  `cargo fmt --all -- --check` — pass; `codegg validate --config
  codegg.example.jsonc` — **"Configuration is valid"**; the affected guards
  and their self-tests (`check_queue_write_transaction_shape`,
  `check_shared_checkout`, `check_writer_disposition`, `check_process_denial`,
  `check_scheduler_bypass`, `check_execution_ownership`,
  `check-core-boundary`) — pass; and **143 tests** across the M005 writer
  suites and the three merged scheduler binaries, 0 failed:
  `shared_checkout_c006` 16, `process_denial_c005` 18,
  `checkout_reservation_c004` 14, `writer_fence_c003` 10,
  `writer_disposition_guards` 5, `scheduler_executor_coverage` 1,
  `writer_permit_service` 12, `tool_broker_integration` 25,
  `scheduler_contention` 14, `scheduler_phase5` 13,
  `scheduler_restart_recovery` 15.
* `scripts/verify.sh quick` — pass (exit 0).
* `cargo fmt --all -- --check` — pass.
* `cargo clippy --workspace --all-targets -- -D warnings` — pass.
* `tests/shared_checkout_c006.rs` — **16 tests**, 0 failed (the sixteenth is
  the composed carrier test of §8.3).
* Regression suites, all green (64 tests in total, 0 failed):
  `process_denial_c005` (18), `checkout_reservation_c004` (14),
  `writer_fence_c003` (10), `writer_disposition_guards` (5),
  `scheduler_executor_coverage` (1), `shared_checkout_c006` (16).
* Guards: `check_shared_checkout.py` (+ `--self-test`),
  `check_process_denial.py` (+ `--self-test`),
  `check_writer_disposition.py` (+ `--self-test`),
  `check_audit_coverage.py` (+ `--self-test`), `check_adr_integrity.py`,
  `check-core-boundary.sh`, `check_daemon_cwd_usage.py` — all pass.

### 8.2 What was NOT run, and why

* **A full-workspace nextest sweep on this host** could not complete:
  another session's `cargo nextest run --workspace --locked --profile ci`
  holds the cargo lock and the load is ~10. The hosted runs above are the
  full-workspace evidence, and the scoped suites are the local evidence.
* **No macOS or Windows run was possible on this host.** The macOS lane
  (§5.1) exists to produce that evidence on a hosted runner; Windows has no
  backend to run (§5.1).
* **The branch has since been reconciled with `origin/main`.** The hosted run
  above was dispatched on the branch head (`workflow_dispatch` accepts any
  ref), so it was evidence about **this branch's head**, not about a merge
  result — which is why the merge reconciliation is a separate, named piece of
  work and not something this record's runs were asked to cover. It has since
  been done: merge commit `42707515b` reconciles the branch with
  `origin/main` `9c9ffb763` (20 files conflicted in 72 hunks), and is recorded
  in §9 of
  `plans/closure/agent-workstream-m005-writer-containment-corrective/004-merge-reconciliation-status.md`.

### 8.3 The composed scenario

The plan's Q1/Q2 ask for a composed run through a real daemon, scheduler and
broker with two agents racing for one checkout. **The carrier half of that is
now composed** (`two_members_placed_through_the_real_scheduler_share_one_checkout`,
in `tests/shared_checkout_c006.rs`):

* the **real** `JobScheduler` and `JobSubmissionService`;
* the **real** `SubagentJobExecutor`, registered with the daemon's own
  `SubagentAuthority` — the same `SharedCheckoutCoordinator` and the same
  `DaemonWriterFenceFactory` the daemon builds;
* two **real** `JobPayload::SubagentRun` jobs carrying
  `placement = SerializedShared { group_id }`, one per group member;
* the **real** coordinator and the **real** factory resolving the fence.

Both members are attached to the group's checkout by the executor's shared
branch, and the two fences the daemon resolves for them name that same
checkout and queue behind each other on its writer permit: the second writer
cannot take the checkout until the first settles.

What the composed test deliberately does **not** do is start a child. The
pool is built with no agents, so the run stops after the placement. That is
the point — the placement happens before the agent is looked up, and an
unknown agent is treated as isolation-requiring, which is what routes the run
into the placement branch at all.

So the claim is now: **the production carrier places two members of one group
on one checkout, and the daemon's fences for them exclude each other.** The
claim that is still *not* made is that two *live* agents have been observed
issuing real writes through the broker inside one daemon process. That needs
a live model and a contained Git backend, and it stays a narrow follow-up.

### 8.4 The regressions the hosted CI found

The hosted runs found two real defects that the local scoped suites could not
have found, both introduced by C005 and both invisible to a run that only
touches the corrective's own suites.

**The frozen M003 tool-advisor receipt.** The receipt test reconstructs the
M003-era tool schemas by reversing every recorded post-M003 change — the
scratch fields, the claim fields, the action enum, the identifier shapes and
the `path` descriptions — and asserts the reconstructed catalog still
fingerprints to the stored receipt. C006 added the `task` tool's `placement`
parameter, which is one more post-M003 change, and the reconstruction did not
know about it. The fix is the one the test's own design calls for:
`placement` is removed from the reconstructed historical schema, with an
assertion that it is present, so the receipt stays immutable and the live
schema keeps the opt-in placement. Nothing about the receipt itself changed.

**The branch did not compile on Windows.** The Windows native probe workflow
failed to compile the `codegg` lib while `main` compiles the same files
cleanly. Both errors came from C005: the C005 admission check in
`src/interactive_process.rs` sat between the `#[cfg(not(unix))]` early return
and the `#[cfg(unix)]` handle creation, so on Windows the check's `if let`
became the function's tail expression; and the test-only Windows AppContainer
launcher in `src/managed_process.rs` destructured `ManagedProcessRequest`
without listing the two fields C005 had added. The first is fixed by making
the check Unix-only — the interactive PTY is refused outright on every other
platform, so there is no child for the profile to deny — and by letting the
`#[cfg(not(unix))]` block's `return` give the block the `never` type. The
second is fixed by listing and ignoring the two fields. Windows behaviour is
unchanged in both cases: the refusal there is the platform's, not the
profile's.

Both are recorded because they are the reason a full-workspace hosted run is
not a formality. C005 was committed with no hosted CI run at all, and two
platform-specific defects shipped with it.

### 8.5 The failure that is not C006's — and is now fixed

The second hosted run's single failure was
`scheduler_contention::different_exclusivity_keys_run_in_parallel`: two jobs
with *different* exclusivity keys must run concurrently, and the test's
`Barrier::new(2)` makes each of them wait for the other, so a scheduler that
serialises them trips the test's five-second budget.

That is the per-workspace build admission — **M007's** feature, not M005's.
The test fails at the branch's **pre-session** head as well, so it was
pre-existing on this branch rather than something C006 introduced. It was
attributed, not dismissed: the third hosted run (`39627444733`, head
`2cddbff3c`) reproduced it unchanged — 9,780 of 15,650 tests passed, 1
failed, and the failure was the same test.

The **cause** was found during the merge reconciliation with `main`, and it
is a contract interaction, not a bug in either side:

* The test submits two `JobKind::Build` jobs into **one** workspace with
  different exclusivity keys, and asserts they run in parallel.
* The submission boundary **unconditionally** binds the build-class quota to
  the canonical workspace (`KeyedQuotaRequest::for_workspace`), so both jobs
  land in the same counted scope.
* That scope's default limit is **one build slot per workspace**
  (`scheduler.quota.max_concurrent_builds_per_workspace`, default `1`).

So the two jobs were serialised by the **quota** — a different admission
dimension from the one the test names — and the test failed while asserting
nothing about exclusivity at all. The fix widens the quota bound **in that
test only**, so the test measures the dimension its name names. The shipped
contract is unchanged: one build slot per workspace by default is still what
`architecture/build_admission.md` says it is, and the quota's own contract is
covered by the build-admission suite.

The alternative — lowering the shipped default to two builds per workspace —
would have changed the product contract to satisfy a test, which is the
wrong direction. The test passes in the merged tree
(`scheduler_contention` 14/14).

The merge reconciliation itself is recorded in §9 of
`plans/closure/agent-workstream-m005-writer-containment-corrective/004-merge-reconciliation-status.md`.

---

## 9. Known limits and non-claims

1. **Windows is disabled.** The gate keeps the profile off any platform
   without native containment evidence. Adding a platform to
   `QUALIFIED_SHARED_CHECKOUT_PLATFORMS` is a recorded decision with
   evidence behind it, never a quiet edit — and macOS was added on exactly
   that basis (§5.1). Windows is disabled for the stronger reason: it has no
   filesystem-enforcement backend at all, so the evidence cannot be produced
   by any run.
2. **A live two-agent race is not claimed.** The carrier is composed and
   proven (§8.3); two live agents issuing real writes inside one daemon
   process are not, and that is a narrow follow-up.
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
