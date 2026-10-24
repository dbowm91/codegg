# Agent Workstream M005 Writer-Containment Corrective C007 — Shared-Checkout Placement Correctness: Status

Status: closed

Milestone: `plans/implementation/agent-workstream-m005-writer-containment-corrective/007-shared-checkout-placement-correctness.md`
Review baseline: `f1b83a7a7` (`origin/main`, 2026-10-23) — the tree §3's evidence was gathered against
Implementation baseline: `f2639b5d2` (`origin/main`, 2026-10-23) — the tree this work was applied to

## 1. Executive finding

Two correctness defects, on the shared checkout's two lifecycles.

**The group-level placement never reached the children.** `spawn_many` computed the effective placement — the child's own placement, falling back to the group's — and then used it for exactly one thing: deciding whether to insert the `group_id`. It was never written back into the child object. The nested spawn therefore re-read `input["placement"]`, found the key absent, and parsed `Auto`. The documented group-level form was inert: a `spawn_many` call with `placement: serialized_shared` placed its children on **Auto**, with the full tool profile, on whatever checkout the default path chose. The review reproduced it by execution: children got `placement=Auto denied_tools=[]`, while the per-request control passed with `SerializedShared{group_id}` and the full denied list.

**The allocating member released the group's shared lease when it ended.** The executor captured the lease only on the coordinator's `Created` outcome, and released it unconditionally on both teardown paths. Releasing it set `owner_run_id = NULL` and `state = 'ready'` — and the writer fence reads `owner_run_id`: with it `None`, the authorization match falls through to `_ => false`. The surviving member's write was refused with `NotAuthorized`, the group's reservation lapsed, and the group could split across two checkouts. **No test exercised this**; `is_creator()` appeared only as a negative assertion.

Two more defects fell out of the same review and are closed here: a **store read error** during the group's checkout resolution was collapsed into "skip this candidate", so a transient failure returned `Ok(None)` and `acquire` allocated a **second** checkout for a group that already held one; and a **shared placement that could not be honoured** was dropped in favour of the parent's workspace root — with no writer fence, because the run's durable record carried no `worktree_id`.

## 2. Requirement-to-evidence matrix

| §13 criterion | Evidence | Result |
|---|---|---|
| 1. A `spawn_many` call with a group-level `placement: serialized_shared` places both children on **one** shared checkout, with the shared narrowing tool profile, and neither child receives an isolated checkout | §3.1 and §4.3 | **met** — the placement reaches the child in the parsed form; the one-checkout property is the coordinator's existing contract, unchanged and still covered by `two_members_placed_through_the_real_scheduler_share_one_checkout` |
| 2. The member that allocated the checkout ending does not strip the surviving member's write authority, does not lapse the reservation, and does not cause a second checkout | §3.2 and §4.1 | **met** — the lease is held while the group is live |
| 3. A store read error during the group's checkout resolution produces a typed refusal and allocates no second checkout | §3.3 and §4.2 | **met** |
| 4. A shared placement that cannot be honoured is refused with a typed error and never silently executed in the parent's workspace | §3.4 | **met, with one named divergence — §10** |
| 5. Every defect above has a regression test that fails against the baseline and passes after the fix | §4 | **met** |
| 6. Every new guard is wired into `verify.sh quick` or CI and has a `--self-test` case that is rejected when the defect is reintroduced | §5 | **met** |
| 7. `verify.sh quick`, workspace clippy and the named test targets are green | §4.4 and §4.5 | **met** — `verify.sh quick` is green across its 86 steps at the closing commit, the workspace clippy with `-D warnings` is green, and the four named test targets are green |

## 3. Production implementation evidence

### 3.1 The group-level placement reaches the child

`src/tool/task.rs` — `spawn_many`'s per-child loop now writes the effective
placement back into the child object, in the **parsed** form:

```rust
let effective_placement = object
    .get("placement")
    .and_then(|value| value.as_str())
    .or_else(|| input["placement"].as_str());
let placement = match effective_child_placement(
    effective_placement,
    group_id_text.as_str(),
) {
    Ok(placement) => placement,
    Err(error) => {
        // C007 §6.4: an unhonourable placement is
        // reported as a rejected child, never as an
        // accepted one, and never leaves the group
        // half-created.
        rejected.push(format!("child {index}: {error}"));
        continue;
    }
};
object.insert(
    "placement".into(),
    serde_json::Value::String(placement.as_str().to_string()),
);
```

The rule is now a named function, `effective_child_placement`, so it is
directly testable: the **child's own** placement wins, the **group's**
placement is the fallback, and the value is the parsed form — so the nested
spawn reads exactly the canonical string the parser accepts and the shared
narrowing is applied.

The child's own `group_id` insert now keys off `placement.is_shared()`
rather than a string comparison against `"serialized_shared"`.

### 3.2 The shared lease belongs to the group

Three coordinated changes.

**The coordinator owns the disposition.**
`crates/codegg-core/src/shared_checkout.rs` gained:

```rust
pub async fn group_is_live(
    &self,
    group_id: &AgentRunGroupId,
) -> Result<bool, SharedCheckoutError>

pub async fn release_group_lease(
    &self,
    checkout: &SharedCheckout,
) -> Result<bool, SharedCheckoutError>
```

`release_group_lease` releases the lease **only** when the group has reached a
terminal status, and returns whether it did. A live group keeps its lease.

**The executor asks the coordinator.**
`src/scheduler/executors.rs` — the teardown path now has two release helpers:
`release_isolated_checkout`, which releases the lease on a checkout the run
holds **for itself** and returns at once when the run holds a shared checkout,
and `release_shared_checkout`, which delegates to
`coordinator.release_group_lease(checkout)` and holds the lease when the group
is live. The isolated path is unchanged: an isolated checkout's lease is
released exactly as it always was.

The executor also tracks the group's checkout separately from the lease it
holds, because a **joiner** holds no lease of its own and must still be able
to release the group's lease once the group is done.

**The reconcile path does not strip a held lease.**
`src/core/runtime_deps.rs` — `DurableWorktreeOwnerResolver` now takes the
daemon's run-group service. A run that has ended is still "terminal" for the
reconcile path's purposes only if it is not holding a checkout **on behalf of a
live group**. Without this, a reconcile pass would release the lease the
moment the allocating member ended, and the fix would hold only until the
next reconciliation.

### 3.3 The resolution fails closed on a store error

`crates/codegg-core/src/shared_checkout.rs` — `resolve_with_group` previously
collapsed a store error into "skip this candidate":

```rust
let Ok(Some(run)) = runs.get_run(&member).await else { continue; };
let Ok(record) = worktrees.get(&worktree_id).await else { continue; };
```

Both are now distinguished. A candidate that simply does not exist is still
skipped — **absence is a fact**; a failed read is **not**, and now produces the
typed `SharedCheckoutError::Lookup`. The coordinator refuses rather than
continuing to the next candidate, so a transient read failure cannot make a
group split across two checkouts.

### 3.4 An unhonourable shared placement is refused, not dropped

`src/scheduler/executors.rs` — the shared branch is now entered on the
**placement**, not on `needs_isolation` and not on the repository identity
being present. The run is refused with a typed error when the placement
cannot be honoured:

- no durable run identity → `"a shared placement must use the durable scheduler path"`;
- no managed-worktree service → `"a shared placement requires managed worktree isolation"`;
- no repository identity (a repo-less root) → `"a shared placement cannot be honoured: the workspace has no repository identity, so there is no managed checkout to share"`, with the run's failure class `shared_checkout_unavailable`.

Before this change, all three of those paths fell through to the parent's
workspace root with **no writer fence**, because the run's durable record
carried no `worktree_id` and the fence factory returned `Ok(None)`.

The placement's own unhonourability at the task-tool level — an unknown
placement, or a shared placement on a platform without native containment
evidence — is reported as a **rejected child**, never an accepted one, and
never leaves the group half-created.

## 4. Verification executed

All commands were run **locally**. No hosted lane was consulted, and none is claimed.

### 4.1 The lease is held while the group is live

`tests/shared_checkout_c006.rs` gained
`the_groups_lease_is_held_while_the_group_is_live`:

- the fixture's group is live (two active members);
- `coordinator.release_group_lease(&checkout)` returns `false`;
- the checkout's record still names the lease holder.

And `the_groups_lease_is_released_when_the_group_has_finished`:

- both members are advanced through the run state machine
  (`Created -> Queued -> Preparing -> Running`) and finished, and the group is
  recomputed after each member's terminal transition;
- `group_is_live` is `false`;
- `release_group_lease` returns `true`;
- the record's `owner_run_id` is `None` — the checkout returns to the pool.

The state-machine walk is not decoration. The run state machine does not
allow `Created -> Completed`, and `finish` refuses an invalid transition
rather than forcing a terminal status. The test therefore reaches the
terminal state the way the executor does, so the group's terminal status is
observed as a **fact about the group** rather than manufactured by a
transition the product would have rejected.

Against the baseline, the first test has no counterpart: the baseline released
the lease unconditionally when the allocating run ended.

#### 4.1.1 A defect this milestone's first implementation carried, and the fix

The two tests above exercise the **coordinator** directly. They did not
exercise the **executor**, and the executor's ordering was wrong: it decided
the shared lease's disposition *before* the run's terminal transition was
durable. At that moment the group still looked live, so
`release_group_lease` returned `false` and the lease was **retained for
good** — the checkout never returned to `ready`, and the group held its
reservation forever. The production path did not release the lease at all.

Two changes close it, and both are needed:

- **The release is decided after the terminal transition.** The executor's
  teardown now calls `release_isolated_checkout` before the transition and
  `release_shared_checkout` **after** it. The isolated path is unchanged: its
  lease is the run's own, and it is released exactly as it always was.
  The shared path's lease belongs to the group, so its disposition cannot be
  decided while the run is still live.
- **The group's liveness is derived from its members' durable statuses**, not
  from the group's stored status. The stored status is recomputed by a
  separate notification path, so at the moment of the release it may still
  say `running` even though every member has finished. A member whose durable
  record is **gone** cannot be running; a member whose record cannot be
  **read** is an unresolved liveness, and the lease is held.

`the_groups_lease_is_released_even_when_the_groups_stored_status_is_stale`
covers the second change: every member's durable run is finished, the group
is deliberately **not** recomputed — so its stored status is stale — and the
lease is still released.

`two_members_placed_through_the_real_scheduler_share_one_checkout` covers
the first change end to end: it places two members through the real
scheduler, waits for both to complete, and now also asserts that the
checkout's lease is released and that the record is back to `ready`.

### 4.2 A failed lookup is a typed error

`tests/shared_checkout_c006.rs` gained
`a_failed_lookup_while_resolving_the_groups_checkout_is_a_typed_error`: a
group whose member's durable record names a checkout that is not in the store.
The resolution must produce `SharedCheckoutError::Lookup`, not `Ok(None)` and
not a checkout. Against the baseline the same tree produced `Ok(None)`, because
the error was collapsed into "skip this candidate".

### 4.3 The placement reaches the child

`src/tool/task.rs` gained two unit tests:

- `the_group_level_placement_reaches_the_child` — the child's own placement
  wins over the group's; an unknown placement is a typed error, not a silent
  `Auto`;
- `the_shared_placement_carries_the_groups_identity` — the shared placement
  carries the group it is bound to, and where the shared profile is
  unavailable the refusal is still typed rather than a silent `Auto`.

Against the baseline, the group-level form produced `Auto` for every child,
which is what the review reproduced by execution.

### 4.4 The named test targets

§10 and §11 of the plan name the shared-checkout suite. The scoped run on
the implementation tree:

| Target | Result |
|---|---|
| `cargo test -p codegg --test shared_checkout_c006` | **19 passed, 0 failed** |
| `cargo test -p codegg --test contained_worktree_c008` | **7 passed, 0 failed** |
| `cargo test -p codegg --features server --test core_daemon` | **507 passed, 0 failed** —
  **498** without the `server` feature, which gates the nine
  `work_orders_m005_trigger::http_fire` tests |
| `cargo test -p codegg --test writer_fence_c009` | **4 passed, 0 failed** |

`shared_checkout_c006` was **16** tests before this milestone and is **19**
after: the three added by this milestone. The C008 corrective's own tests live
in `tests/core_daemon/checkout_mediation_authority_c008.inc` and
`tests/server_feature/checkout_mediation_authority_c008_http.inc`, so this
suite's count is C007's alone.

The same scoped run also executed the `server_feature` target, which reports
**100 passed, 1 failed**. That failure is
`projection_transport_real::real_tui_pending_snapshot_disconnect_convergence_50_cycles`,
a **pre-existing** flake in a file this milestone does not touch; it was
measured on the untouched baseline and its analysis is in
`plans/closure/agent-workstream-m005-writer-containment-corrective/008-status.md`
§10 (U1).

### 4.5 The whole-tree gates

`scripts/verify.sh quick` is **green** on the implementation tree at the
closing commit — 86 steps,
including `cargo fmt --check --all`, every guard with its `--self-test`, and
`cargo check --workspace --all-targets --locked`: every crate and every test
target compiles. `cargo clippy --workspace --all-targets --locked -- -D warnings`
is **green** as well, after four lints the new code introduced were fixed.

## 5. Guards

This milestone added no new guard script of its own: the defects it closes
are *runtime* defects, and the guards that protect them are the ones the
adjacent milestones added and which this milestone's changes interact with:

- `scripts/check_writer_fence_completeness.py` — asserts that the nested
  dispatch forwards the parent's fence and that the worker's containment-only
  fallback is guarded by a managed-checkout probe. C007's shared-lease
  disposition is what makes that forwarding meaningful: the surviving member
  still holds the lease, so the forwarded fence can take the permit.
- `scripts/check_checkout_mediation_authority.py` — asserts that the
  reservation authority does not resolve a path through a capped list and that
  the gate and the effect share one workspace-identifier rule. C007's §6.4
  refusal uses the same rule.
- `scripts/check_shared_checkout.py` — the existing C006 contract guard, whose
  `src/scheduler/executors.rs` landmark is `c006-shared-placement`, the marker
  C007's shared branch still carries.

All three run in `scripts/verify.sh quick` and in CI, each with its
`--self-test`.

## 6. Invariant review

§4's seven invariants, reviewed against the implementation tree:

| # | Invariant | Status |
|---|---|---|
| 1 | Containment precedes the permit — a target outside the run's checkout is refused before any permit is taken | **intact** — `src/core/writer_fence.rs` is unchanged by this milestone |
| 2 | The group's share is derived from durable state, never asserted by the caller | **intact and strengthened** — the lease's *disposition* is now derived from the group's durable status at teardown, not asserted by the executor |
| 3 | `Auto` is the default everywhere; nothing grants `SerializedShared` implicitly | **intact** — the change only makes an *explicitly requested* placement reach the child. A caller that asks for nothing still gets `Auto` |
| 4 | The three group refusals stay distinct — `UnknownGroup`, `GroupNotLive`, `NotAMember` | **intact** — the coordinator's group refusals are unchanged; `group_is_live` is a read-only snapshot seam and adds none |
| 5 | A non-member is refused before any checkout is created | **intact** — the coordinator's membership check is unchanged |
| 6 | The shared profile narrows the tool profile to the tools that cannot reach a process, and the narrowing must not become optional on the shared path | **intact** — the narrowing is applied where it always was; C007 makes the *placement* that selects the shared path actually reach the child, which is what makes the narrowing reachable at all |
| 7 | The fence's authorization is not weakened to accommodate placement — a member that does not hold the lease is refused | **intact and strengthened** — the lease is now *held* for the group's lifetime rather than dropped, so a surviving member is authorized by the existing `shares_live_group` arm. No new arm was added and no existing arm was weakened |

## 7. Failure, cancellation, restart and contention semantics

**A live group's lease is held.** The allocating member ending no longer
releases it. The group's final member ending — or the group reaching a
terminal status by any other route — releases it, and the checkout returns to
`ready`.

**A restart does not strip it.** The lease is durable
(`managed_worktree.owner_run_id`), so the checkout still resolves to the group
after a restart, and the reconcile path no longer releases a lease whose owner
has ended but whose group is still live.

**An unreadable group state holds the lease.** If the group cannot be read at
teardown, the lease is **held** rather than released: releasing it on an
unreadable state would strip the group's members of their write authority. The
reconcile path releases it once the owner run is terminal and the group is no
longer live.

**A store error refuses.** The resolution surfaces `Lookup` instead of
continuing. The group keeps whatever checkout it already had; no second
checkout is allocated.

**An unhonourable placement refuses.** The run fails with a typed error and
the failure class `shared_checkout_unavailable`. The child never executes in
the parent's workspace without a fence.

## 8. Compatibility and migration review

No migration. No schema change.

The durable payload's **shape** is unchanged: `RunPlacement` was already the
payload's placement type, and the baseline already stored `Auto` for a
group-placed child. What changed is the **value** — a child that the caller
explicitly placed on the shared profile now records `SerializedShared{group_id}`
instead of `Auto`. That is the correction the milestone exists to make, and it
is a value change inside an existing type, not a shape change.

The executor's teardown path changed shape internally (`owned_worktree` gained
a sibling, `shared_checkout`); no public API changed.

## 9. Security review

The security-relevant direction is the one this milestone restores: **a member
of a live group keeps its write authority**, and a member that does not hold
the lease is still refused.

Before this milestone, the lease's release *removed* the authority of a member
that was still entitled to it, and — because the lease was gone — a group
could allocate a second checkout and operate on two checkouts at once, with
neither checkout's permit recognising the other. That is the split the fence
exists to prevent.

The other direction is unchanged: no fence arm was added, and the
`shares_live_group` arm still requires both the requesting run and the lease
owner to be members of a **live** group.

## 10. Unresolved findings

**U1 — the plan's WP4 case (a) is *honoured* here, not refused.** The plan's
WP4 named two cases: (a) an agent that denies every mutating tool, so
`requires_isolation` is false, and (b) a repo-less workspace. This
implementation refuses case (b) with a typed error. For case (a) it **honours**
the placement: the run joins the group's shared checkout, gets a
`worktree_id`, and therefore a working fence.

The reasoning: the plan's governing sentence is *"a shared placement is either
honoured or refused, never dropped"*, and its §13 criterion 4 says *"A shared
placement that cannot be honoured is refused with a typed error and never
silently executed in the parent's workspace."* Case (a) **can** be honoured —
the coordinator joins the existing group checkout without allocating a second
one — so refusing it would reject a configuration that is safe: the run holds
no mutating tool, so it cannot write into the checkout it joins. Before this
milestone, case (a) was neither honoured nor refused: it was silently dropped
in favour of the parent's workspace root with no fence, which is the defect.

**This is a deliberate divergence from WP4's example, and the maintainer can
reverse it** with a one-line change to the shared branch's entry condition
(`needs_isolation || wants_shared_checkout` → `needs_isolation`). Severity:
low. The safe direction is preserved either way, because the child's own
workspace root is never substituted for the group's.

**U2 — the whole-tree verification gap opened by the disk emergency is closed.**
The host's disk reached 100% during this session (the shared XDG trash had
accumulated 287 GB of already-trashed build caches; purging it was necessary to
make the tree buildable), and the scoped tests and the guards were prioritised
over the whole-tree gates. Both gates have since been run on this tree:

- `scripts/verify.sh quick` — **green**, 86 steps on the implementation tree:
  `cargo fmt --check --all`,
  every guard with its `--self-test`, and
  `cargo check --workspace --all-targets --locked`;
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — **green**
  after four lints the new code introduced were fixed.

Severity: **none**. The finding is kept in the record because the gap was real
when this milestone was written, and a reader comparing the record with the
tree's history needs to see that it was closed by measurement rather than by
assumption.

**U3 — the delegated child's transport principal is still unbound.** A
delegated run's child loop carries no turn identity, so it resolves no
transport principal of its own; its write on a team daemon is refused (the
fail-closed direction) and on a single-user daemon it is attributed to the
local fallback. This is recorded by the C009 corrective, which owns the
principal binding, and is named here because C007's shared-lease disposition
is what gives the delegated child something to hold.

## 11. Roadmap disposition

The agent workstream M005 writer-containment corrective line moves from
`C001–C006 closed (C006 conditionally, its condition now C007's closure); C007–C009 ready` to
`C001–C009 closed`.

**C007, C008 and C009 closed in the same commit.** The line's three corrective
milestones were independent of one another, so they were implemented and
closed together rather than in sequence; the registry therefore records the
line's final state once, and each closure record names the transition the
line made in that commit.

**This milestone discharges M005's condition.** M005 moved from *conditionally
closed* to *closed strictly* at the registry reconciliation recorded in §12,
because the condition was C007's closure and this record is it.

C008 and C009 own defects on the mediation authority and the agent-loop fence
respectively; both closed in the same commit, and neither was discharged by
this milestone alone.

## 12. Registry updates

`plans/registry.md` records, in this same commit:

- the agent workstream M005 writer-containment corrective row's status cell →
  `C001–C009 closed`;
- the M005 row's status cell in the closure table → `closed` (from
  `conditionally closed`), with the condition named as this milestone's
  closure;
- the "Dependency-ready implementation plans" entry for this plan → moved out
  of dependency-ready, with its closure record named;
- a new closure-table row for `C007` on the agent workstream M005
  writer-containment corrective line.

## 13. Handoff notes

The next reader should know three things.

**The lease belongs to the group, and the group is derived from durable
state.** The coordinator's `release_group_lease` is the single place the
disposition is decided; the executor asks it; the reconcile path respects it.
Three places had to agree, and all three did.

**The reconcile path was the quiet half.** Making the executor hold the lease
would have bought nothing if the next reconciliation released it — which it
would have, because the owner run had ended. The group-aware
`DurableWorktreeOwnerResolver` is what makes the fix durable.

**The placement fix is a value fix, not a shape fix.** The payload type never
changed; the value the child carries did. A group-placed child now records
what the caller asked for instead of the default.

## 14. Amendment: implementation review

An implementation review of this milestone's change set found defects that the
verification in §4 did not reach, because the tests exercised the coordinator
directly and never the executor's ordering. The record is amended rather than
replaced: the closure stands, and the amendments below are what the review
changed.

**A1 — the shared lease was never released in production.** The executor
decided the shared lease's disposition **before** the run's terminal
transition was durable. At that moment the group still looked live, so
`release_group_lease` returned `false` and the lease was retained for good —
the checkout never returned to `ready`. §4.1's two tests called the
coordinator directly, so they passed while the production path was broken.

The fix has two parts, and both are needed: the executor releases the
**isolated** lease before the transition and the **shared** lease after it
(`release_isolated_checkout` / `release_shared_checkout` in
`src/scheduler/executors.rs`), and `group_is_live` derives the group's
liveness from its **members' durable run statuses** rather than from the
group's stored status, which is recomputed by a separate notification path
and may still say `running` when every member has finished.

**A2 — `release_group_lease` reported a success it could not verify.** The
release's error was discarded (`let _ = ...`), so a failed release was logged
as a released lease. The error is now propagated.

**A3 — two refusal arms left the run un-finished.** The "no shared-checkout
coordinator" arm and the "shared checkout linkage failed" arm returned a
failure without recording the terminal transition, so the run stayed in
`Preparing` for good — and a group whose member never finishes never
terminates, which holds the shared lease. Both arms now finish the run, as
their neighbours do.

**A4 — `group_is_live` failed open on an unreadable member.** A member whose
durable record cannot be read is an unresolved liveness, not an absent one:
the answer is an error and the lease is held. A member whose record is
**gone** cannot be running and does not keep the group alive.

The second half of that is safe in every reachable case, and the review
checked it: the group record is created *after* the children it names are
spawned, so a member's durable record always exists before the group names
it, and there is no run-deletion path in the tree. A model-supplied member
id with no record names a run that will never join, so treating it as
terminal is right — and a **forward** reference, which is the shape that
would let the lease go early and a later member allocate a second checkout,
is not reachable.

**A5 — the shared-checkout guard's coverage.** The guard set that protects
this milestone grew by one rule — `fail-closed-authority-read` in
`scripts/check_writer_fence_completeness.py` — which asserts that the fence's
durable authority reads are never answered with "absent". See the C009
record's §14 for the rule itself.

**A6 — the guard set changed, and the step count did not.** The review
removed a duplicated `check_writer_fence_completeness` invocation from
`scripts/verify.sh` (the guard ran twice in `run_guards`, −2 steps) and wired
`check_daemon_cwd_usage.py`, which this line's C008 corrective extended but
which was wired into **neither** `verify.sh quick` nor CI (+2 steps). The
count is therefore unchanged at **86 steps** over a different guard set — the
count at the branch tip `8225f42dd`; the merged tree's count is in §14 A8.

**A7 — §3.3's quoted code.** The two lines quoted in §3.3 paraphrase the
baseline's error-swallowing form; the tree's form is a `match` on the store
read. The substance of §3.3 — absence is skipped, a failed read is a typed
`SharedCheckoutError::Lookup` — is unchanged and was verified against the
tree.

**A8 — the merged tree's measurements.** The **86 steps** recorded above were
measured on the branch tip `8225f42dd`, where the quick lane's `==>` lines were
83 in `run_guards` plus 3 in `run_quick`. Reconciling with `origin/main`
`3b29c5b88` adds **nine** — `origin/main`'s four reasoning-control guards
(eight lines: each runs once with `--self-test` and once for real) and the
harness-lab unittest step (one) — so the tree merged at `d5b7efc02` runs
**95**
`==>` lines. The count is measured, not incremented:

```bash
awk '/^run_guards\(\)/,/^}/' scripts/verify.sh | grep -c 'echo "==>'   # 91
awk '/^run_quick\(\)/,/^}/' scripts/verify.sh | grep -c 'echo "==>'    # 4
```

That tree's quick lane was re-run after the merge: **green**, 95 steps.

The named test targets moved with the review pass, so §4.4's table keeps the
closing-commit numbers and the live ones are: `shared_checkout_c006` is
**20** (the review added the lease-release regression test) and
`writer_fence_c009` is **5** (the fence factory's unreadable-store refusal).
`core_daemon` is **508** with `--features server` and **499** without it, both
re-measured on the merged tree.

**A9 — the fourth reconciliation with `main`.** `origin/main` advanced again
while this line was open: the **repository audit corrective** workstream landed
at `6e097dd3a`, and the branch tip `d5b7efc02` had to be reconciled with it.
The quick lane's `==>` lines moved again, from 95 to **109** — `run_guards` is
**105** and `run_quick` is **4** — because `origin/main` added its own guards
and its own generated-asset checks. The count is measured, not incremented:

```bash
awk '/^run_guards\(\)/,/^}/' scripts/verify.sh | grep -c 'echo "==>'   # 105
awk '/^run_quick\(\)/,/^}/' scripts/verify.sh | grep -c 'echo "==>'    # 4
```

The merged tree's quick lane was re-run after this merge: **green**, 109
steps.

**A10 — the fifth reconciliation with `main`.** `origin/main` advanced again
while this line was open: the **file-picker surface corrective** workstream
landed at `c2d781319` (PR #182), and the branch tip `a5c00ef10` had to be
reconciled with it. The quick lane's `==>` lines moved again, from 109 to
**111** — `run_guards` is **107** and `run_quick` is **4** — because
`origin/main` added its own `/files` keymap-drift guard, which runs twice
(once with `--self-test` and once for real). The count is measured, not
incremented:

```bash
awk '/^run_guards\(\)/,/^}/' scripts/verify.sh | grep -c 'echo "==>'   # 107
awk '/^run_quick\(\)/,/^}/' scripts/verify.sh | grep -c 'echo "==>'    # 4
```

The merged tree's quick lane was re-run after this merge: **green**, 111
steps.

The merge's only code-level interaction was additive: this branch touched
no `src/tui/` source, so `main`'s file-picker changes are orthogonal to
every corrective on this line. The counts that moved — *Integration test
files* **338**, *Guard scripts (all)* **91**, *CI guard scripts* **54** — are
recorded in `architecture/overview.md` and
`architecture/count_audit_history.md`, and the comment budget was re-banked
on the merged tree at **123,685** lines, still below the 124,865
pre-reduction baseline.

**A11 — the sixth reconciliation with `main`.** `origin/main` advanced again
while this line was open: the **composer's second post-closure corrective**
workstream landed at `a4bbba81e` (PR #183), and the branch tip `28ebe7163` had
to be reconciled with it. The quick lane's `==>` lines moved again, from 111 to
**113** — `run_guards` is **109** and `run_quick` is **4** — because
`origin/main` added its own composer keymap-drift guard, which runs twice
(once with `--self-test` and once for real). The count is measured, not
incremented:

```bash
awk '/^run_guards\(\)/,/^}/' scripts/verify.sh | grep -c 'echo "==>'   # 109
awk '/^run_quick\(\)/,/^}/' scripts/verify.sh | grep -c 'echo "==>'    # 4
```

The merged tree's quick lane was re-run after this merge: **green**, 113
steps.

The merge's only code-level interaction was additive: `main` changed no file
outside `src/tui/`, and this branch changed no file under `src/tui/`, so the
two sides' code is disjoint and `main`'s composer changes are orthogonal to
every corrective on this line. The counts that moved — *Guard scripts (all)*
**92** and *CI guard scripts* **55** — are recorded in
`architecture/overview.md` and `architecture/count_audit_history.md`.
*Integration test files* is **338** and did **not** move: `main`'s composer
corrective extended the existing members `tests/tui_render/tui_composer_claims.inc`
and `tests/tui_render/tui_composer_motions.inc` and rewrote the already-existing
`tests/tui_render/tui_composer_selection.inc`, so it added no new integration
test file. The comment budget was re-banked on the merged tree at **124,310**
lines / **1,095,646** words / ratio **0.1330**, still below the 124,865
pre-reduction baseline.
