# M005 Writer-Containment Corrective C007 — Shared-Checkout Placement Correctness

Status: ready for handoff
Repository baseline: `f1b83a7a7` (`origin/main`, 2026-10-23)
Source roadmap: `plans/subsystems/agent-workstream-blackboard-m005-writer-containment-corrective-addendum.md`
Long-term requirements: `plans/000-long-term-specification.md` §§9, 16–17, 26–29; `plans/001-terminology-and-domain-model.md` §§5, 7–8
Applicable ADRs: ADR-0002 (agent run worktree isolation and control, accepted), ADR-0032 (agent workstream coordination and shared checkout ownership, accepted), ADR-0047 (restricted agent shared checkout writer contract, accepted option B)
Primary class: invariant

## 1. Objective

Make one claim true end to end:

> **A group that asks for a shared checkout runs on exactly one shared checkout, that checkout holds exactly one lease for as long as the group is live, and the placement the caller asked for is the placement that runs — or the run is refused with a typed error.**

This milestone closes four defects found in the 2026-10-23 implementation review of the blackboard line. Two of them are majors that break the claim outright; two are minors that break it in narrower configurations.

The claim is about **placement mechanics**, not model quality and not the fence itself. The fence contract, the permit store and the coordinator's authority model are already correct and are not reopened here.

## 2. Why this milestone is ready

- **C006 is closed.** `plans/closure/agent-workstream-m005-writer-containment-corrective/006-status.md` qualified the restricted shared checkout; its last non-claim (the live two-agent race) was discharged by `plans/closure/agent-workstream-blackboard/011-status.md`.
- **Every defect below is in production code that already has a carrier.** The coordinator, the scheduler executor, the task tool and the fence are all production paths exercised by `tests/shared_checkout_c006.rs`.
- **Two of the four defects were reproduced by execution** during the review, with a passing control that isolates each from its harness. The other two were established by code tracing across the coordinator's real consumers.
- **No new architecture is required.** Every fix below is a change to an existing decision point inside an existing contract. No ADR is needed; the placement contract is already carried by ADR-0032 and ADR-0047.

## 3. Current implementation evidence

### 3.1 The group-level `placement` is computed and then discarded (major)

`src/tool/task.rs:1588-1597` computes the *effective* placement for each `spawn_many` child — the child's own `placement`, falling back to the group's top-level `placement` — and uses it **only** to decide whether to insert `group_id`:

```rust
let placement = object
    .get("placement")
    .cloned()
    .unwrap_or_else(|| input["placement"].clone());
if placement.as_str() == Some("serialized_shared") {
    object.insert("group_id".into(), serde_json::Value::String(group_id.to_string()));
}
```

`object` is `child.as_object_mut()` (`src/tool/task.rs:1578-1579`), so the `group_id` insert does reach the child. The effective `placement` is **never written back**. The nested dispatch at `src/tool/task.rs:1599` re-enters `execute_impl` with the child object, and the nested spawn reads `input["placement"]` at `src/tool/task.rs:1868-1869` — where the key is absent — so it parses `RunPlacement::Auto`. Because the tool-profile narrowing at `src/tool/task.rs:1870-1873` is gated on `placement.is_shared()`, it does not apply either.

**Executed evidence.** The review drove `task` with `action=spawn_many`, a top-level `placement: serialized_shared`, and two requests without their own `placement`. The group was created with both children as members (`Members: 2 … active 2`), yet each child's submitted job carried `placement=Auto denied_tools=[]`. The control — the same harness with the placement on each request instead of the group — produced `SerializedShared{group_id}` and the full denied list `["bash","ci_watch","formatter","lsp","lsp_read","python_script","terminal","test","tool_program","verify"]`.

**Consequence.** The children take the isolated branch (`src/scheduler/executors.rs:1154-1192`), each receives its own checkout, and each keeps every process-capable tool. The configuration the tool schema advertises — `src/tool/task.rs:1367-1370`: *"It is honoured for spawn_many, where the group is created with the children as members"* — is unreachable. Only the per-request form works today.

### 3.2 The member that allocated the shared checkout releases the lease when it ends (major)

`src/scheduler/executors.rs:1140-1147` captures the checkout **only** on the `Created` outcome:

```rust
if let codegg_core::shared_checkout::SharedCheckoutOutcome::Created { record, lease, .. } = outcome {
    owned_worktree = Some((*record, *lease));
}
```

The success path (`src/scheduler/executors.rs:1418-1429`) and the error path (`:1506-1517`) both then call `service.release_lease(&record.worktree_id, &lease.owner_run_id, lease.generation)`, whose store update is `UPDATE managed_worktree SET owner_run_id = NULL, state = 'ready'` (`crates/codegg-core/src/worktree_service.rs:819`).

Nothing else holds the lease: the group stays live, and `recompute_group` runs only on create/status/wait/list.

**Three consequences follow from the real code.**

1. **The surviving member loses write authority.** `DaemonWriterFence::begin` reads `record.owner_run_id` (`src/core/writer_fence.rs:284`); with it `None` the authorization match falls through to `_ => false` (`:285-288`) and returns `WriterFenceError::NotAuthorized`, surfaced to the model as a permission refusal (`src/tool/writer_fence.rs:305-310`).
2. **The group's C005 reservation lapses.** `reservation_for_record` returns `Unreserved` for an unleased checkout (`crates/codegg-core/src/checkout_reservation.rs:224-227`), so the "a shared checkout admits no unqualified process for the whole group" guarantee stops being derived for a checkout a member is still using.
3. **The group can split across two checkouts.** `resolve_with_group` skips a candidate whose record has no `owner_run_id` (`crates/codegg-core/src/shared_checkout.rs:229-231`) and returns `Ok(None)` (`:259`), so `acquire` allocates a **second** checkout for a group that already holds one, with no shared writer permit between them.

**Executed evidence.** The review reproduced (1) directly: a member's write was refused with `Err(NotAuthorized { root: "/tmp/codegg-c006-repro-lease-repository-…" })` while the group was live and the member was still working in the checkout. (3) was code-traced: the managed-checkout creation path needs a contained Git backend the review's test tree did not have.

**Why no test caught it.** The `Created` branch is the only path that sets `owned_worktree` for a shared checkout, and it is the branch production takes for a group's first member. `is_creator()` appears in `tests/shared_checkout_c006.rs:499` only as a **negative** assertion. Every fixture pre-acquires the lease by hand, so the release path was never modelled.

### 3.3 A store read error is indistinguishable from "the group has no checkout" (minor)

`crates/codegg-core/src/shared_checkout.rs:220` and `:226` collapse `Err` and `Ok(None)`:

```rust
let Ok(Some(run)) = … else { continue; };
let Ok(record) = … else { continue; };
```

If every candidate errors — a transient SQLite failure, say — `resolve_with_group` returns `Ok(None)` (`:259`) and `acquire` allocates a **new** checkout for a group that already holds one. That is the same group split as §3.2(3), triggered by a read error rather than a lease release. This is a fail-open in an authority check.

### 3.4 A shared placement is silently not honoured (minor)

The shared branch in `src/scheduler/executors.rs` sits inside `if needs_isolation { … if let Some(repository_id) = task.repository_id.clone() { … } }` (`:961`, `:989`, `:1059-1065`, `:1089`). With `placement = SerializedShared`, a shared placement is silently dropped when either:

- **(a)** the agent's permissions deny every mutating tool, so `requires_isolation` is false (`src/agent/worker.rs:658-690`); or
- **(b)** the workspace has no `repository_id` (a repo-less root) — the code logs *"executing in place without worktree isolation"* (`:1060-1063`).

In both cases the child runs in the **parent's** workspace root with **no writer fence**, because the fence factory resolves from the run's durable record, which has no `worktree_id`, and returns `Ok(None)` (`src/core/writer_fence.rs:529-541`). The requested placement is dropped rather than refused — the same failure the coordinator's own doc forbids (`crates/codegg-core/src/shared_checkout.rs:279-282`).

## 4. Invariants that must not regress

1. **Containment precedes the permit.** A target that resolves outside the run's own checkout is refused before any permit is taken (`src/core/writer_fence.rs:260-267`).
2. **The group's share is derived from durable state, never asserted by the caller** (`src/core/writer_fence.rs:295-297`).
3. **`Auto` is the default everywhere.** `RunPlacement::default() == Auto`; the durable field is `#[serde(default)]`; the legacy `JobPayload::Subagent` arm hardcodes `Auto` (`src/scheduler/executors.rs:883`); nothing grants `SerializedShared` implicitly.
4. **The three group refusals stay distinct**: `UnknownGroup`, `GroupNotLive`, `NotAMember`, each returned on its own input.
5. **A non-member is refused before any checkout is created.**
6. **The shared profile narrows the tool profile to the tools that cannot reach a process.** This narrowing is derived from the capability table (`src/tool/process_capability.rs:169-185`) and must not become optional on the shared path.
7. **The fence's authorization is not weakened to accommodate placement.** Every fix below must preserve §3.2's consequence (1): a member that does not hold the lease is refused.

## 5. Scope

**In scope**

- The `spawn_many` group-level placement propagation in `src/tool/task.rs`.
- The shared checkout's lease lifetime in `src/scheduler/executors.rs` and the coordinator's resolution in `crates/codegg-core/src/shared_checkout.rs`.
- The shared branch's entry condition in `src/scheduler/executors.rs`, and the typed refusal that replaces a silently-dropped placement.
- The store-error disposition in `crates/codegg-core/src/shared_checkout.rs`.
- Regression tests for each of the four defects, and the guards in §10.

**Out of scope**

- The fence authority chain, the permit store's state machine and the idempotency key. These were reviewed and are correct; C008 and C009 of this line are the owners if a future change touches them.
- The non-broker writer sinks (C004's subject) — C008 of this line owns them.
- The worker's containment-only fence fallback for nested dispatch — C009 of this line owns it.
- The contained managed-worktree creation grant — the Linux trusted Git line's C009 owns it.
- Any change to the WorkOrder `shared`/`serialized` mutation policy, which still fails closed. Enabling it is a product decision with no registered owner.
- The ADR-0032 shared-write mode. It is **not enabled** by this milestone.

## 6. Required production changes

### 6.1 Propagate the effective placement into the `spawn_many` child

In `src/tool/task.rs`, the effective placement must reach the child object, not merely the `group_id` decision. The value written must be the *effective* one — the child's own when present, the group's otherwise — and it must be the same value the nested spawn parses.

The narrowest correct change is to insert the effective placement next to the existing `group_id` insert, using the parsed form so the child carries a canonical value rather than the raw string. If the effective placement is shared, the `group_id` insert stays exactly as it is.

### 6.2 Hold the shared checkout's lease for the group's lifetime

The shared checkout's lease must not be released by the member that allocated it while the group is still live. The change belongs in the executor's teardown, where `release_lease` is currently called unconditionally for `owned_worktree`.

Two acceptable dispositions, in preference order:

1. **Hold the lease.** When the run that allocated a shared checkout ends and its group is still live, the lease is retained and the checkout remains the group's. The lease is released when the group reaches a terminal status.
2. **Transfer the lease.** The lease is reassigned to a remaining live member of the group before the allocating run ends.

Disposition 1 is preferred: it is the smaller change and it keeps one durable owner for the group rather than moving authority between runs. If disposition 1 is taken, the group-completion path must release the lease, and that path must be covered by a test.

Either way, the executor must distinguish a **shared** checkout from an **isolated** one before releasing. An isolated checkout's lease is released exactly as today — that is not in scope and must not change.

### 6.3 Fail closed on a store read error during resolution

`crates/codegg-core/src/shared_checkout.rs:220` and `:226` must distinguish `Err` from `Ok(None)`. A store error while resolving a group's checkout must not be treated as "no checkout". The existing `SharedCheckoutError` vocabulary should carry a disposition for it (an existing unusable-checkout variant, or a new one) and the coordinator must surface it rather than continuing to the next candidate.

### 6.4 Enter the shared branch on the placement, and refuse when it cannot be honoured

The shared branch must be entered on `placement.is_shared()`, not on `needs_isolation` and not on `repository_id` being present. When a shared placement cannot be honoured — no repository identity, or no isolated execution available — the run must be **refused with a typed error**, not silently executed in the parent's workspace.

A shared placement that is refused must not leave a group half-created. The group-creation path in `src/tool/task.rs` must report the refusal as a rejected child rather than an accepted one.

## 7. Ordered work packages

### WP1 — Propagate the group-level placement

**Intent:** make the documented `spawn_many` group-level placement reach the child.

**Required changes:** §6.1.

**Acceptance evidence:** a `spawn_many` call with a group-level `placement: serialized_shared` and two children without their own placement produces two children whose durable payload carries `SerializedShared{group_id}` and whose effective tool profile is the shared narrowing list. The per-request form must keep working unchanged.

### WP2 — Hold the shared lease for the group's lifetime

**Intent:** a live group's shared checkout keeps exactly one lease holder.

**Required changes:** §6.2.

**Acceptance evidence:** in a group of two shared members, the member that allocated the checkout ends while the other is still running; the surviving member's write is **not** refused with `NotAuthorized`; the checkout's reservation is still derived; and the group still resolves to one checkout.

### WP3 — Fail closed on a store error during resolution

**Intent:** a transient store failure cannot make a group split across two checkouts.

**Required changes:** §6.3.

**Acceptance evidence:** a store error injected during `resolve_with_group` produces the typed error, and no second checkout is allocated for the group.

### WP4 — Refuse an unhonourable shared placement

**Intent:** a shared placement is either honoured or refused, never dropped.

**Required changes:** §6.4.

**Acceptance evidence:** with `placement = SerializedShared` and (a) an agent that denies every mutating tool, and (b) a repo-less workspace, the run is refused with a typed error, the group records the refusal, and no child executes in the parent's workspace without a fence.

### WP5 — Guards and regression suite

**Intent:** the four defects cannot recur silently.

**Required changes:** §10.

**Acceptance evidence:** every guard is wired into `scripts/verify.sh quick` or CI, and each guard has a `--self-test` case that must be rejected.

## 8. Failure, cancellation, restart and contention semantics

- **The allocating member fails or is cancelled.** The shared checkout's lease is retained under disposition 1 of §6.2. The group is not split, and the surviving member keeps its write authority.
- **Every member ends.** The group's final teardown releases the lease, and the checkout returns to `ready` in the store (`crates/codegg-core/src/worktree_service.rs:819` sets `owner_run_id = NULL, state = 'ready'`). The store's existing reconciliation path must cover this.
- **The daemon restarts with a live group holding a shared checkout.** The lease is durable (`managed_worktree.owner_run_id`), so the checkout still resolves to the group after restart. The existing reconcile path must not release a lease whose owner is a run in a live group.
- **Two members race for one group's checkout.** Unchanged: the coordinator's per-group lock covers resolve → preflight → create (`crates/codegg-core/src/shared_checkout.rs:272-300`), and `tests/shared_checkout_c006.rs::two_members_racing_for_one_group_end_up_on_one_checkout` covers it.
- **A store read error during resolution.** Now refused, not treated as "no checkout" (§6.3).
- **A placement that cannot be honoured.** Refused before the child starts (§6.4); no partial group.

## 9. Compatibility and migration

- **No schema change is expected.** The lease is already durable in `managed_worktree.owner_run_id`; §6.2 changes its *lifetime*, not its shape. If WP2 turns out to need a column, it must be an additive migration with the schema-bound and index conventions the roadmap's cross-cutting section requires, and the migration's version must be recorded.
- **The durable payload shape is unchanged.** `RunPlacement` already carries `SerializedShared{group_id}`; §6.1 makes the existing field carry the value the schema already advertises. Payloads written before this change with `Auto` keep meaning `Auto`.
- **Older payloads lacking a placement field keep defaulting to `Auto`.** `#[serde(default)]` on the durable field must be preserved.
- **A group created before this change and still live.** Its checkout may already have an unleased record if the allocating member ended under the old behaviour. That group resolves to no checkout under the new coordinator and will be allocated a fresh one. This is a one-time consequence of the §3.2 defect and must be recorded in the closure record; it is not a migration.

## 10. Required tests

**Focused unit tests**

- `crates/codegg-core/src/shared_checkout.rs` — a store error during resolution yields the typed error, not `Ok(None)`; a candidate with no `owner_run_id` is skipped; a candidate whose lookup errors is not skipped.
- `src/tool/task.rs` — the effective placement is the child's own when present and the group's otherwise; the value written to the child is the parsed form.

**Integration tests** (extend `tests/shared_checkout_c006.rs` or add a sibling target)

1. `a_group_level_placement_reaches_every_child` — the `spawn_many` reproduction: group-level `serialized_shared`, two children, both durable payloads carry `SerializedShared{group_id}` and the shared narrowing list.
2. `the_per_request_placement_still_works` — the control, unchanged.
3. `a_live_group_keeps_its_shared_lease_when_the_allocating_member_ends` — the §3.2 reproduction: the surviving member's write succeeds, the reservation is still derived, and the group still resolves to one checkout.
4. `an_isolated_checkout_still_releases_its_lease` — the non-regression for disposition 1 of §6.2.
5. `a_store_error_during_resolution_does_not_allocate_a_second_checkout`.
6. `a_shared_placement_without_a_repository_identity_is_refused` — case (b) of §3.4.
7. `a_shared_placement_without_isolation_is_refused` — case (a) of §3.4.

**Static guards** (new, each with a `--self-test`)

- A guard asserting the `spawn_many` group-level placement is written into the child object, and that the nested spawn's placement parse is reachable from the group-level value.
- A guard asserting the executor's shared-checkout teardown does not release a lease whose group is live — or, if disposition 2 is taken, that the lease is transferred rather than released.
- A guard asserting the shared branch is entered on `placement.is_shared()`.

## 11. Required verification commands

Run from the repository root at the milestone's baseline.

```bash
cargo fmt --all -- --check
CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=2 bash scripts/verify.sh quick
CARGO_BUILD_JOBS=2 cargo test --locked -p codegg --test shared_checkout_c006
CARGO_BUILD_JOBS=2 cargo test --locked -p codegg-core --test writer_permit
```

If a new integration target is added, it must be listed in `scripts/verify.sh quick` and in `.github/workflows/ci.yml` alongside its siblings, and the new guards must be wired into `scripts/verify.sh quick` with their `--self-test` invocations.

**Never** run `--all-features` for a workspace sweep; it drags in the real-server tests.

## 12. Documentation updates

- `plans/subsystems/agent-workstream-blackboard-m005-writer-containment-corrective-addendum.md` — the milestone table gains C007; the status line records the open corrective work.
- `plans/subsystems/agent-workstream-blackboard-roadmap.md` — its status line notes that the M005 corrective line carries open C007–C009. **M005 itself stays closed**; this milestone corrects the shared-checkout implementation, it does not reopen M005's scope.
- `plans/closure/agent-workstream-blackboard/008-status.md` — **read-only**: a closure record is immutable except for factual correction. M008's matrix row A cited the `task` tool's `placement` parameter as a user-reachable path; §3.1 shows the group-level form of that parameter is inert. The correction is recorded in this milestone's closure record (§7), not by editing M008's record.
- `architecture/writer_permit.md` — if §6.2 changes the lease's lifetime, the documented lease lifetime and the group-completion release path must be updated here.
- `architecture/blackboard_contract_matrix.md` — if the contract matrix names the shared-checkout placement path, it must name the group-level form and say which form carries it.
- `docs/` — no user-facing change is expected. The placement parameter's behaviour is unchanged for every caller that already used the per-request form; the group-level form begins to work as documented.

## 13. Acceptance criteria

Externally observable, and each must be demonstrated:

1. A `spawn_many` call with a group-level `placement: serialized_shared` places both children on **one** shared checkout, with the shared narrowing tool profile, and neither child receives an isolated checkout.
2. In a two-member shared group, the member that allocated the checkout ending does not strip the surviving member's write authority, does not lapse the checkout's reservation, and does not cause a second checkout to be allocated.
3. A store read error during the group's checkout resolution produces a typed refusal and allocates no second checkout.
4. A shared placement that cannot be honoured is refused with a typed error and never silently executed in the parent's workspace.
5. Every defect above has a regression test that fails against the baseline and passes after the fix.
6. Every new guard is wired into `scripts/verify.sh quick` or CI and has a `--self-test` case that is rejected when the defect is reintroduced.
7. `verify.sh quick`, `cargo clippy --workspace --all-targets -- -D warnings` and the named test targets are green.

## 14. Stop conditions

Report rather than improvise if:

- **§6.2 disposition 1 requires a schema change** that the roadmap's cross-cutting storage conventions do not already permit. Report the required shape before migrating.
- **Holding the lease (§6.2) turns out to conflict with the store's existing `release_lease` semantics** — for example if `release_lease` is also the path that marks a checkout `ready` after a run ends and the two cannot be separated. Report the coupling; do not change the store's contract to fit the executor.
- **The group-completion release path cannot be identified** — if no durable record shows when a group reaches a terminal status, disposition 1 is not implementable and disposition 2 (transfer) must be chosen instead. Report that before proceeding.
- **§6.4's typed refusal requires a new error variant** that the protocol or the client surface must carry. Report the surface change before adding it.
- **The fix for §3.1 changes the durable payload shape** beyond carrying the existing `RunPlacement` value. Report it; the payload shape is a compatibility surface.
- **Any fix would weaken an invariant in §4**, in particular invariant 7.

## 15. Closure evidence required

1. A closure record at `plans/closure/agent-workstream-m005-writer-containment-corrective/007-status.md`, with the sixteen-section structure the closure template requires.
2. A requirement-to-evidence matrix mapping each of §13's seven criteria to the test or command that demonstrates it.
3. The exact verification commands and their results, labelled honestly as local or hosted. A milestone MUST NOT be marked closed when the required tests were not run and no justified substitute exists.
4. The two executed reproductions from the review — the group-level placement and the lease release — re-run against the fixed tree and shown to pass.
5. An invariant review confirming §4's seven invariants are intact, quoting the guard or test that shows each.
6. A registry update in the same commit as the closure, per the planning process's closure-commit rule.
7. A disposition for M008's matrix row A, recorded in this closure record rather than by editing M008's immutable closure record.

## 16. Handoff notes

- The four defects were found by a 2026-10-23 review of `origin/main` at `f1b83a7a7`. Two were reproduced by execution; two were established by code tracing. Both kinds of evidence are recorded in §3.
- The tree at `f1b83a7a7` is **green**: `tests/shared_checkout_c006.rs` (16), `tests/contained_worktree_c008.rs` (5), `tests/composed_two_agent_race_m011.rs` (3), `codegg --lib tool::writer_fence` (3) and `codegg-core --test writer_permit` (21) all pass. These defects are latent, not caught. Do not treat the green suite as evidence against §3.
- §3.2 is the highest-value item: it is the only defect that lets a live group's shared checkout lose its lease and split the group. Start with WP2 if the milestone must be executed partially, and record the partial state honestly.
- C008 and C009 of this line are the sibling corrective milestones for the mediation authority and the agent-loop fence respectively. They are independent of this one and must not be folded in.
- The shared checkout remains **opt-in** and **platform-gated**. Nothing here enables the WorkOrder `shared`/`serialized` mutation policy, which still fails closed.
