# Agent Workstream Blackboard M008 — Strict Closure

Status: closed — comparative trajectory and economics qualification, with the shared-checkout arm (S4) qualified on Linux and macOS and unrun on Windows

Source plan: `plans/implementation/agent-workstream-blackboard/008-comparative-trajectory-qualification.md`
Implementation commits: `4d327add9` (the qualification suites), `3477bc589` (the frozen matrix and the guard), `3549d9216` (the counts re-measured on the base this branch lands on)
Pull request: [#160](https://github.com/dbowm91/codegg/pull/160) — merged to `main` as `e1b898cb8` on 2026-10-22
Hosted CI on the PR head: [`39733201090`](https://github.com/dbowm91/codegg/actions/runs/39733201090) — success, **16,059 tests run: 16,059 passed, 8 skipped**, 0 failed (521.966s workspace-tests step)

## 1. Result

M008 closes with two things.

The first is the **frozen contract matrix**. The plan's work package A asked for
the contract matrix to be frozen before any two configurations are compared
against each other, because a comparison is only meaningful when every arm
runs the same contract. That freeze is now a document
(`architecture/blackboard_contract_matrix.md`) and a guard
(`scripts/check_blackboard_contract_matrix.py`), and it names, for each of the
six capabilities the blackboard added, the `codegg-core` authority that owns
it and the **production** call site that consumes it. A capability with no
production consumer is not a capability, it is a label, and the guard says so.

The second is the **qualification of the five configurations** against that
matrix, with the measured comparison published beside it. S0 performs no
blackboard work at all. S1's worktree isolation is unchanged. S2 and S3's peer
observations attribute the actual peer action, arrive in source order, and stay
untrusted coordination data rather than becoming instructions. S3's claim
arbitration yields exactly one owner without widening a scope. S4 places two
children on one shared checkout with the real writer fence serializing them,
and both children's commits are Git-verified in that one checkout. The
explicit integration returns an actionable conflict rather than an unqualified
automatic success.

Two claims M008 does **not** make, both recorded as named non-claims in §9: no
live-model performance claim, and no live two-agent race inside one daemon
process.

## 2. The frozen contract matrix (work package A)

`architecture/blackboard_contract_matrix.md` names six capabilities, the
authority that owns each, the production consumer that exercises it, the
user-reachable path, and the evidence:

| Capability | Authority | Production consumer |
|---|---|---|
| group join | `crates/codegg-core/src/agent_run_group.rs` | `src/tool/task.rs` — `.create(NewAgentRunGroup {` |
| blackboard claim | `crates/codegg-core/src/workstream_claim.rs` | `src/tool/task.rs` — `.claim(codegg_core::workstream_claim::NewWorkstreamClaim {` |
| peer event | `crates/codegg-core/src/bus/events.rs` | `src/agent/follow_up.rs` — `fn inject_peer_observations` |
| session-worktree launch | `crates/codegg-core/src/session_launch.rs` | `src/core/session_launch.rs` — `pub async fn launch(` |
| group lease | `crates/codegg-core/src/worktree_service.rs` | `src/core/writer_fence.rs` — `async fn shared_group(` |
| build optimization | `crates/codegg-core/src/jobs/quota.rs` | `src/scheduler/submission.rs` — `apply_resource_policy(&mut spec);` |

`scripts/check_blackboard_contract_matrix.py` is the guard, and it is wired
into `scripts/verify.sh quick` and the CI `verify` job. It fails when:

* one of the six capabilities is missing from the matrix;
* a row names a consumer file that does not exist;
* a row names a marker that is no longer present in that file;
* a row names a marker short enough to match by accident (minimum 12
  characters);
* a row names an empty marker; and
* a row's consumer marker exists **only inside a test-only module** — the
  consumer has retreated out of production code, and the matrix still claims
  it has one.

The last clause reuses the cfg-stripper that
`scripts/check_writer_disposition.py` already validates, so a consumer gated
by `#[cfg(test)]`, `#[cfg(all(test, ...))]` or any equivalent predicate is
recognised as test-only.

The guard's self-test proves it is not vacuous. It rejects a missing
capability row, a missing file, a vanished marker, a too-short marker, an
empty marker, a marker that lives only in a test module (planted in a **copy**
of a real consumer file inside a temporary tree, so the working tree is never
written to), a matrix with no table, and a malformed row — and accepts the real
matrix.

**The matrix pins file + marker, not line numbers.** A line number in a frozen
table is a point-in-time fact that goes stale the moment an unrelated line is
inserted above it, and a stale point-in-time fact in a frozen contract is
worse than none. The exact lines the markers were read at are recorded in
§3 below.

## 3. Where the production consumers were read

The matrix was written against the tree at `012ece80a` (the M005
writer-containment line merged into `main` as `53d9fc9fd`). Every consumer was
read in that tree:

| Capability | File | Line | Consumer |
|---|---|---|---|
| group join | `src/tool/task.rs` | 1674 | `.create(NewAgentRunGroup {` |
| blackboard claim | `src/tool/task.rs` | 689 | `.claim(codegg_core::workstream_claim::NewWorkstreamClaim {` |
| peer event | `src/agent/follow_up.rs` | 61 | `fn inject_peer_observations` |
| session-worktree launch | `src/core/session_launch.rs` | 133 | `pub async fn launch(` |
| group lease | `src/core/writer_fence.rs` | 168 | `async fn shared_group(` |
| build optimization | `src/scheduler/submission.rs` | 315 | `apply_resource_policy(&mut spec);` |

## 4. Requirement-to-evidence matrix

| Plan requirement | Evidence | Result |
|---|---|---|
| A — freeze the contract matrix; every named capability has a production consumer | `architecture/blackboard_contract_matrix.md`; `scripts/check_blackboard_contract_matrix.py` and its self-test | pass |
| A — enumerate the supported CLI/TUI/daemon/agent paths | The matrix's "User-reachable path" column; the `codegg launch` CLI at `src/main.rs:2281`, the TUI worktree actions, and the `task` tool's `placement` parameter at `src/tool/task.rs:2325` | pass |
| S0 — one capable agent stays operational and pays nothing for the blackboard | `tests/blackboard_trajectory_m008.rs::s0_a_single_agent_performs_no_blackboard_work_at_all` | pass |
| S1 — two independently mutating children on separate worktrees do not cross before integration | `tests/blackboard_trajectory_m008.rs::s1_two_children_in_separate_worktrees_do_not_see_each_others_edits` | pass |
| S2/S3 — observations attribute the actual peer action, in source order, coalesced | `tests/agent_loop_harness.rs::m008_peer_observation_contract::peer_observations_arrive_in_source_order_and_duplicates_are_coalesced` | pass |
| S2/S3 — a peer payload never becomes an instruction | `tests/agent_loop_harness.rs::m008_peer_observation_contract::a_hostile_peer_payload_is_carried_as_data_and_never_becomes_an_instruction` | pass |
| S0/S2 — the optional no-blackboard path stays free | `tests/agent_loop_harness.rs::m008_peer_observation_contract::a_single_agent_outside_the_peers_group_receives_no_peer_observation_at_all` | pass |
| S3 — a claim race has exactly one owner and never widens a scope | `tests/blackboard_trajectory_m008.rs::s3_a_claim_race_has_exactly_one_owner_and_never_widens_a_scope` | pass |
| S3 — a replayed request never creates a second owner | `tests/blackboard_trajectory_m008.rs::s3_replaying_a_claim_request_never_creates_a_second_owner` | pass |
| S3 — an outsider's claim is refused with a typed error | `tests/blackboard_trajectory_m008.rs::s3_a_claim_by_an_outsider_is_refused_with_a_typed_error` | pass |
| B — observations stay bounded through cancel | `tests/blackboard_trajectory_m008.rs::s3_a_cancelled_member_board_is_explicitly_unavailable` | pass |
| B — source fact ordering | `tests/blackboard_trajectory_m008.rs::s3_claim_events_are_ordered_and_bounded` | pass |
| C — explicit integration of disjoint work leaves a clean checkout | `tests/blackboard_trajectory_m008.rs::an_explicit_integration_of_disjoint_work_leaves_a_clean_checkout` | pass |
| C — an overlapping integration returns an actionable conflict, not an automatic success | `tests/blackboard_trajectory_m008.rs::an_overlapping_integration_returns_an_actionable_conflict` | pass |
| S4 — two children on one shared checkout serialize their writes | `tests/blackboard_trajectory_m008.rs::s4_two_children_on_one_shared_checkout_serialize_their_writes` | pass on Linux; qualified on macOS by the C006 native-containment lane; **unrun on Windows** |
| S4 — the platform gate is exactly the qualified table | `tests/blackboard_trajectory_m008.rs::the_shared_profile_is_available_exactly_on_the_qualified_platforms` | pass |
| E — the group notification is claimed exactly once across a restart | `tests/blackboard_trajectory_m008.rs::e_a_group_notification_is_claimed_once_across_a_restart` | pass |
| E — the group delivers its members in a stable ordinal order | `tests/blackboard_trajectory_m008.rs::e_the_group_delivers_its_members_in_a_stable_ordinal_order` | pass |
| D — a measured comparison with published denominators, resource costs and negative cases | `tests/blackboard_economics_m008.rs::the_configuration_comparison_publishes_its_denominators` | pass (opt-in; §6) |
| D — at least one Rust target-dir contention measurement | `tests/blackboard_economics_m008.rs::the_rust_target_dir_contention_is_measured_and_published` | pass (opt-in; §6) |
| B — group/child and worktree lifecycle may not leak permits or overwrite unrelated filesystem state | the S4 arm's permit is taken and settled, never leaked; the integration tests assert a clean `git status --porcelain` after every merge and abort, and the worktrees live outside the repository so the assertion measures the integration rather than the fixture | pass |

## 5. The fault/replay coverage that already existed (work package B)

M008 did not write a new fault suite from scratch: the plan's fault list is
mostly covered by the milestones it compares. The coverage map, and what M008
added on top:

| Fault the plan names | Where it was already covered | What M008 added |
|---|---|---|
| simultaneous claim CAS | `tests/workstream_claim_m003.rs::simultaneous_overlapping_claims_have_one_winner_and_stale_cas_fails` | the race under concurrency, with the scope-widening assertion |
| cross-tenant authorization | `tests/workstream_claim_m003.rs::nonmember_actor_is_denied_and_schema_survives_reopen` | the outsider refusal asserted from the product's own error type |
| SQLite close/reopen | the same test | — |
| malicious peer text | `src/agent/follow_up.rs` peer-observation unit tests | the hostile payload carried through the **real provider boundary** |
| source fact ordering | — | the source-order and coalescing assertions |
| duplicate events | — | the coalescing assertion |
| incomplete stored snapshots | `crates/codegg-protocol/src/projection/workstream.rs` | a cancelled member's board reads as **explicitly unavailable**, not as an empty board |
| cancellation | `tests/scheduler_restart_recovery.rs`, `tests/scheduler_contention.rs` | the cancelled-member board refusal |
| process-tree kill | `tests/managed_process*.rs`, the C005 process-denial suite | — (C005's scope) |
| oversize model context | `src/agent/follow_up.rs::oversized_peer_observation_is_item_truncated_without_breaking_json` | — |
| duplicate events / conflicting modified files | `tests/workstream_claim_m003.rs` | the overlapping-integration conflict test |
| sandbox approval reduction | `tests/process_denial_c005.rs`, `scripts/check_process_denial.py` | — (C005's scope) |
| direct-parent messaging | `src/agent/follow_up.rs`, the M004/M009 closures | — |

## 6. The measured comparison (work package D)

The measurement is **opt-in** — `CODEGG_RUN_M008_ECONOMICS=1` — because it
takes real wall-clock time and spawns real cargo processes. CI must not pay
for a measurement nobody reads, and the plan explicitly says not to introduce a
benchmark CI gate.

### The machine

16-core Intel Core i9-9900K, 15 GiB RAM, Linux 6.8.0-147-generic, load
average 2.59/3.86/3.58 at the time of the run. This is a **shared** host, so
the wall-clock numbers carry the noise of whatever else was running. The
report says so in its own `limitations` field.

### The denominators

3 repetitions per configuration per stratum; 3 strata (`single`, `disjoint`,
`overlapping`); 3 configurations (S0, S1, S3); 2 cargo builds per contention
arm.

### The configuration comparison

Wall-clock is the mean over the repetitions, in milliseconds, and covers the
blackboard's own mechanics — the durable writes and the claim arbitration —
not a model turn.

| Configuration | Stratum | Wall mean (ms) | Durable blackboard rows | Duplicate edits | Conflicts |
|---|---|---|---|---|---|
| S0 | single | 1579 | 0 | 0 | 0 |
| S0 | disjoint | 1576 | 0 | 0 | 0 |
| S0 | overlapping | 1898 | 0 | 0 | 0 |
| S1 | single | 1832 | 1 | 0 | 3 |
| S1 | disjoint | 1523 | 1 | 0 | 0 |
| S1 | overlapping | 2756 | 1 | **3** | **3** |
| S3 | single | 3509 | 3 | 0 | 0 |
| S3 | disjoint | 3811 | 5 | 0 | 0 |
| S3 | overlapping | 3960 | 3 | **0** | **0** |

Two findings, one positive and one negative, both published:

1. **On the overlapping stratum, S3 converts a conflicting integration into a
   prevented duplicate edit.** S1 performs 3 duplicate edits and reports 3
   conflicts over 3 repetitions; S3 performs 0 and reports 0, because the
   child that cannot take the claim does not do the work. The cost is 2 extra
   durable rows per repetition and roughly 1.4× the blackboard's own
   wall-clock on that stratum.

2. **The negative case is retained and not hidden.** On the `disjoint`
   stratum, S3 buys **nothing** — S1 also performs 0 duplicate edits and
   reports 0 conflicts — and S3 costs 4 more durable rows and 2.5× the
   wall-clock. The blackboard is overhead with no benefit when two children
   genuinely work on different paths. The plan requires negative results to be
   reported without hiding them, and this is the report.

The `single` stratum shows the same shape from the other side: S3's claim
machinery is pure cost when there is only one child.

### The Rust target-directory contention

Two cargo builds of a generated crate, against one target directory, with the
build script holding the lock for 1200 ms.

| Arm | Per-build wall (ms) | Total | Lock waits observed | Peak RSS (KiB) |
|---|---|---|---|---|
| serialized (M007's counted quota) | 1349, 20 | 1369 | **0** | 18004 |
| concurrent (an unbounded workspace) | 1352, 1372 | 1373 | **2** | 17664 |

The finding is **structural**, not a wall-clock ordering: the serialized arm's
builds never overlap, so they cannot wait for a lock they never contend for;
the concurrent arm's second build waits for the whole of the first and then
finds everything cached. The totals are the same, so admitting the second
build concurrently **buys nothing** — it parks a second cargo process on a
lock. That is the evidence for M007's counted per-workspace quota, and the
suite asserts the structural difference rather than a timing comparison that a
shared host could reorder.

This measurement lives in the **isolated** arms on purpose: the shared profile
refuses every process-capable surface, so a build inside a shared checkout is
refused by contract and could not be measured there at all.

### What the measurement does not claim

The report carries its own `scope` field, and it is explicit: no model
quality, no token cost, no model-turn wall-clock, no live-provider behaviour.
No live provider and no live model exist in this tree, and a report implying
otherwise would be exactly the extrapolation the plan forbids.

## 7. The integration trajectory (work package C)

The integration is **explicit** in both directions:

* disjoint work integrates cleanly with an empty `git status --porcelain`
  afterwards — nothing is left staged, nothing dirty, no file silently
  discarded;
* overlapping work **fails**: the merge exits non-zero, the conflict markers
  are visible in the working tree, `git status` reports the file as
  unresolved, and `git merge --abort` restores the child's own content
  byte-for-byte.

There is no automatic merge anywhere in the path, and no unqualified automatic
success.

## 8. Invariant and security review

* **The shared profile is platform-gated**, and the gate is asserted from the
  product's own table: available on a qualified platform, refused elsewhere
  with a reason that names the platform. The S4 arm reports itself **unrun**
  on an unqualified platform rather than substituting an unsafe emulation.
* **A peer payload never becomes an instruction.** The hostile text is present
  — it is a peer fact, and hiding it would be a different lie — but it is
  present as data: inside the JSON items array, after a preamble that says the
  payload is untrusted, and never as a system message. The trust class is
  `host_observation_hint`.
* **A claim by an outsider is refused** with a typed error and writes
  nothing.
* **A cancelled member's board is explicitly unavailable**, not empty. An
  empty board reads as "there are no claims", which is a different fact from
  "you may no longer read this board".
* **The group notification is claimed durably.** A service rebuilt over the
  same database — a restart — does not claim it again, so live delivery is
  exactly-once across a replay.
* **The writer permit is taken and settled, never leaked.** The S4 arm's
  second writer is refused with `Held` while the first holds the checkout, and
  takes the checkout only after the first settles.
* No new high or medium security or correctness finding was raised by this
  milestone. The one product defect found while working in this area — the
  product-side umask defect in M007's `session_scratch_dir` — is **not**
  M008's, is not fixed here, and is recorded in §9 for the M007 owner.

## 9. Limitations, non-claims and unresolved findings

These are recorded as **non-claims**, not as blockers. Each is a limit on what
M008's evidence supports, and each is a limit the plan itself names.

1. **No live-model performance claim.** The comparison measures the
   blackboard's own mechanics. Model quality, token cost and model-turn
   wall-clock are not measured, and no live provider exists in this tree to
   measure them.
2. **No live two-agent race inside one daemon process.** The S4 arm is two
   real writers contending for one real checkout's permit, driven
   sequentially, through the real coordinator and the real daemon writer
   fence. That is the carrier-level claim M005's C006 closed. A live race
   needs a live model and the contained Git backend, which exists nowhere in
   the tree.
3. **S4 is unrun on Windows.** The shared profile has no filesystem-enforcement
   backend registered for Windows at all, so no run could produce the
   evidence. This is a capability gap, not an evidence gap.
4. **Descendant quiescence is claimed on no platform.** C005's preflight
   refuses the *reservation*, not the process.
5. **The WorkOrder `shared`/`serialized` mutation policy still fails closed.**
   M008 does not change it, and no WorkOrder agent is placed on a shared
   checkout; the profile is reached only through the explicit opt-in.
6. **The measurement's sample is small by construction** (3 repetitions, 3
   strata, 3 configurations) and its wall-clock comes from a shared host. The
   denominators are published with every number so a reader can weigh it.
7. **The M007 product-side umask defect** in `session_scratch_dir` is
   recorded and not fixed here: it creates the cache root with
   `create_dir_all`, inheriting the ambient umask, and then refuses it via
   `ensure_private_parent` if it is group- or other-writable — so a user
   with umask `002` gets a `0775` `~/.cache` and the daemon cannot use the
   agent scratch subsystem at all. It belongs to the M007 owner.

## 10. Dependency and unblock audit

**M008 unblocks no new plan.**

* **M005** keeps its two named re-entry conditions — the absent Windows
  native backend, and a live two-agent race inside one daemon process —
  neither of which M008's evidence addresses.
* **The WorkOrder `shared`/`serialized` mutation policy** still fails closed
  and is untouched by this milestone.
* **No ADR is created, proposed or accepted** by M008. The milestone
  qualifies an existing contract; it decides nothing new.
* **No schema migration** was needed. The blackboard's durable state was
  already in place; M008 adds evidence, one architecture document and one
  guard.
* **No production code changed.** The two new test suites and the guard
  exercise the product as it stands. That is the point of a qualification
  milestone: it measures the product, it does not bend it.

### What M008 added to the tree

| Path | Kind | What it is |
|---|---|---|
| `architecture/blackboard_contract_matrix.md` | document | the frozen contract matrix |
| `scripts/check_blackboard_contract_matrix.py` | guard | the guard that keeps it frozen, with a self-test |
| `tests/blackboard_trajectory_m008.rs` | test | the S0–S4 functional arms, the explicit-integration conflict case, the group member order and the notification durability |
| `tests/blackboard_economics_m008.rs` | test | the opt-in measured comparison and the target-dir contention arm |
| `tests/common/blackboard.rs` | test support | the shared fixture both M008 suites use, so the arms measure and assert against the same state |
| `tests/agent_loop_harness.rs` | test | the three peer-observation contract tests, in the harness that owns the scripted provider |
| `scripts/verify.sh`, `.github/workflows/ci.yml` | wiring | the guard runs in `verify.sh quick` and in the CI `verify` job |

## 11. Verification

Run on the tree carrying this closure:

```bash
cargo fmt --all -- --check
CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=2 bash scripts/verify.sh quick
CARGO_BUILD_JOBS=2 cargo test -p codegg --test blackboard_trajectory_m008 --test blackboard_economics_m008
CARGO_BUILD_JOBS=2 cargo test -p codegg --test agent_loop_harness m008
python3 scripts/check_blackboard_contract_matrix.py
python3 scripts/check_blackboard_contract_matrix.py --self-test
CODEGG_RUN_M008_ECONOMICS=1 cargo test -p codegg --test blackboard_economics_m008 -- --nocapture
```

`cargo fmt --all -- --check` passes. `cargo clippy --workspace --all-targets
--locked -- -D warnings` is **clean**. `scripts/verify.sh quick` reports
**"Quick verification passed."** and includes the new guard and its self-test.

The trajectory suite is **18 passed, 0 failed** (13 M008 arms plus the five
`tests/common/secret_scan.rs` unit tests the shared module carries). The
economics suite is **9 passed, 0 failed** without the opt-in (the measurement
arms report themselves unrun) and **9 passed, 0 failed** with it.

The `tests/agent_loop_harness.rs` target reports **71 passed, 1 failed** on
this host. The failure is
`temp_root_write_following_the_offer_needs_no_human`, it is a **parallel
execution** artefact rather than a deterministic failure (the test passes in
isolation), and it was reproduced on the **pristine** tree with M008's three
harness tests stashed — 68 passed, the same 1 failed. It is therefore
pre-existing and not M008's. It is recorded here rather than hidden, and it
belongs to the harness's own ordering, not to this milestone.

The guard reports `blackboard-contract-matrix check ok: 6 capabilities, 6
matrix rows, every consumer present in production code`, and its self-test
reports `blackboard-contract-matrix self-test ok`.

On the hosted runner the two new CI steps — **"Blackboard contract matrix
guard"** and **"Blackboard contract matrix guard self-test"** — both concluded
success, alongside formatting, workspace Clippy and every pre-existing guard.
The workspace-tests step concluded **16,059 tests run: 16,059 passed, 8
skipped**, 0 failed. The eight skips are the repository's own `#[ignore]`
declarations.

## 12. Source verification

Every file:line in §2 and §3 was read in the tree at `012ece80a`, and every
consumer marker was re-confirmed by grep against it.

The milestone was landed on `origin/main` at `8cee65934`, which is past the
M005 merge. The counts in `architecture/overview.md` were therefore **remeasured
on that base** rather than carried over: `main` had added two integration test
files and one guard script since `012ece80a`, so the two file-count rows are the
union of both sides' contributions. The guard re-checks the
consumers on every run, so a matrix that drifts fails CI rather than quietly
describing a tree that no longer exists. The measured numbers in §6 come from
the run recorded in `target/m008-economics/blackboard-m008-economics.json` and
`target/m008-economics/blackboard-m008-target-contention.json`, on the machine
described in §6.
