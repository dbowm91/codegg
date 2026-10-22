# Blackboard Contract Matrix — the frozen comparison baseline for M008

## Purpose

This is the **contract matrix** that milestone M008 freezes before it compares
any two agent configurations against each other.

M008 compares five experiment configurations (S0–S4) on correctness, cost and
wall time. A comparison is only meaningful if every arm runs the *same*
contract: the same group membership rules, the same claim semantics, the same
peer-observation trust class, the same checkout-lease authority, the same build
admission bound. If one arm silently ran a stronger or weaker contract than
another, the measured difference would be an artefact of the harness rather
than a property of the configuration.

So this document names, once:

1. the six capabilities the blackboard added, the **authority** that owns each
   one, and the **production consumer** that actually exercises it;
2. the experiment configurations and the milestone that discharged each one's
   gate;
3. the platform qualification of the shared-checkout profile.

`scripts/check_blackboard_contract_matrix.py` is the guard. It reads the
matrix table below and fails when a capability loses its production consumer,
when the consumer moves to a test-only module, when a row goes empty, or when
one of the six capabilities disappears from the matrix. A matrix that drifts is
not a frozen matrix, and a comparison run against a drifted matrix is not a
comparison.

## Where It Lives

```
architecture/blackboard_contract_matrix.md   # this document
scripts/check_blackboard_contract_matrix.py   # the guard that keeps it frozen
tests/blackboard_trajectory_m008.rs           # the S0–S4 trajectory suite
tests/blackboard_economics_m008.rs            # the opt-in economics measurement
plans/implementation/agent-workstream-blackboard/008-comparative-trajectory-qualification.md
plans/closure/agent-workstream-blackboard/008-status.md
```

## The contract matrix

Every row names a capability that M001–M007 added, the `codegg-core` authority
that owns it, and the **production** call site that consumes it. A capability
with no production consumer is not a capability; it is a label. The guard
requires each row's consumer to be a real, non-test call site.

The consumer column names a file and a **marker**: the distinctive text of the
call site that consumes the capability. The guard pins the pair, not a line
number — a line number in a table like this is a point-in-time fact that goes
stale the moment an unrelated line is inserted above it, and a stale point-in-time
fact in a frozen contract is worse than none. The exact line each marker was
read at is recorded in the closure record, which is where point-in-time facts
belong.

| Capability | Authority | Production consumer | User-reachable path | Evidence |
|---|---|---|---|---|
| group join | `crates/codegg-core/src/agent_run_group.rs` — `AgentRunGroupService` | `src/tool/task.rs` — `.create(NewAgentRunGroup {` | agent → `task` tool, `action=create_group` | `tests/workstream_claim_m003.rs`; closure `001-status.md` |
| blackboard claim | `crates/codegg-core/src/workstream_claim.rs` — `WorkstreamClaimStore` | `src/tool/task.rs` — `.claim(codegg_core::workstream_claim::NewWorkstreamClaim {` | agent → `task` tool, `action=claim` | `tests/workstream_claim_m003.rs`; closure `003-status.md` |
| peer event | `crates/codegg-core/src/bus/events.rs` — `AppEvent::AgentRunGroupUpdated` | `src/agent/follow_up.rs` — `fn inject_peer_observations` | agent turn → peer hint injected at a provider boundary | `src/agent/follow_up.rs:772`; closure `009-status.md` |
| session-worktree launch | `crates/codegg-core/src/session_launch.rs` — `SessionLaunchService` | `src/core/session_launch.rs` — `pub async fn launch(` | `codegg launch` (CLI `src/main.rs:2281`) and the TUI worktree actions | `tests/session_launch_service.rs`; closure `006-status.md` |
| group lease | `crates/codegg-core/src/worktree_service.rs` — `WorktreeService::acquire_lease` | `src/core/writer_fence.rs` — `async fn shared_group(` | agent → `task` tool, `placement=serialized_shared` | `tests/shared_checkout_c006.rs`; closure `006-status.md` (C006) |
| build optimization | `crates/codegg-core/src/jobs/quota.rs` — `ResourceClass::Build` | `src/scheduler/submission.rs` — `apply_resource_policy(&mut spec);` | any build or format job in one workspace | `tests/build_quota_admission.rs`; closure `007-status.md` |

### What "production consumer" means here

A production consumer is a call site in a **non-test** build of the tree that
reaches the capability from a user-reachable path. Concretely, the guard
requires that:

* the named file exists;
* the named marker occurs in that file;
* the marker survives the removal of every test-only module in that file, so a
  consumer that has retreated into a `#[cfg(test)]` module fails the matrix
  rather than passing it.

The guard deliberately does **not** try to prove the capability *works*. That
is the fixture suite's job (`tests/blackboard_trajectory_m008.rs`, and the
per-milestone closures the matrix cites). The guard proves the wiring is still
there, which is the failure mode a static check can honestly see.

### The peer event's trust class is part of the contract

The peer event row carries a clause the other rows do not need: a peer
observation is **untrusted coordination data, not an instruction**. The
producer stamps it `trust_class: "host_observation_hint"` and prefixes it with
an explicit preamble
(`src/agent/follow_up.rs:34`). A comparison arm that let a peer payload become
an instruction would be comparing a different agent, so the matrix records the
trust class as part of the contract rather than as an implementation detail.

## The experiment configurations

The plan's five configurations, and the milestone that discharged each one's
gate. The **status** column is the frozen fact the comparison runs against.

| Configuration | Shape | Gate | Status |
|---|---|---|---|
| S0 | one capable agent, ordinary checkout and compiler handling | none | operational |
| S1 | 2 independently mutating children, separate worktrees, no peer board | M001/M002 | operational; isolation behaviour unchanged |
| S2 | isolated children with factual peer observations, no claim enforcement | M009 | operational |
| S3 | isolated children with peer observations plus atomic task/dependency claims | M003 | operational |
| S4 | 2 children in an explicitly serialized shared worktree, with observations, claims and compilation-aware resource coordination | M005 corrective C006 | qualified on Linux and macOS; unavailable on Windows |

### What S4 is, and is not

S4 is the only configuration that places two children on **one** checkout. It
is reached only by an explicit opt-in — the `task` tool's `placement`
parameter (`src/tool/task.rs:2325`), gated by
`shared_checkout_availability()`
(`crates/codegg-core/src/run_placement.rs:126`). It is never inferred from a
group, a session or a model request.

Three clauses are part of S4's contract and are carried into every comparison
unchanged:

* **The shared profile refuses every process-capable surface.** A build is a
  process, so the restricted v1 cannot run a Rust build inside a shared
  checkout. That is the contract, not a defect, and it means the Rust
  target-dir contention measurement is an **isolated-arm** measurement.
* **A shared checkout admits no unqualified process** for the whole group,
  and the three C005 process surfaces that are not refused pre-effect (the
  local MCP stdio client, the `egglsp` server launch and the hook shell
  adapter) keep their recorded compensating controls.
* **Integration is explicit.** CodeGG never merges on its own. A conflicting
  integration returns a typed conflict that requires recovery.

### The platform matrix for S4

| Platform | Shared profile | Why |
|---|---|---|
| Linux | available | Landlock native containment evidence |
| macOS | available | Seatbelt native containment evidence, on a native macOS runner |
| Windows | unavailable | no filesystem-enforcement backend is registered for Windows at all, so no run could produce the evidence |

The platform gate is a compile-time table
(`QUALIFIED_SHARED_CHECKOUT_PLATFORMS`,
`crates/codegg-core/src/run_placement.rs:94`). Adding a platform to it is a
recorded decision with evidence behind it, never a quiet edit.

## What this matrix does not claim

* **It does not claim the shared checkout is safe on every platform.** The
  matrix records the gate; the gate is the contract.
* **It does not claim a live two-agent race inside one daemon process.** The
  shared profile's carrier is composed at the scheduler and executor level.
  Two live agents issuing real writes inside one daemon process are a
  separate, narrower claim, and M008 does not make it.
* **It does not claim descendant quiescence on any platform.** A process
  started before the reservation can still be writing; C005's preflight
  refuses the *reservation*, not the process.
* **It does not claim a model-level performance win.** The matrix freezes the
  contract the comparison runs against. It says nothing about whether the
  blackboard makes a model faster, slower or cheaper — the economics
  measurement publishes that separately, with its denominators.
* **It does not claim the WorkOrder `shared`/`serialized` mutation policy.**
  That policy still fails closed, so no WorkOrder agent is placed on a shared
  checkout; the profile is reached only through the explicit opt-in.

## Testing

```bash
python3 scripts/check_blackboard_contract_matrix.py --self-test  # prove it bites
python3 scripts/check_blackboard_contract_matrix.py               # check the tree
cargo test --test blackboard_trajectory_m008                      # the S0–S4 trajectory
CODEGG_RUN_M008_ECONOMICS=1 cargo test --test blackboard_economics_m008 -- --nocapture
```

## Source verification

The matrix was written against the tree at
`012ece80a` (the M005 writer-containment line merged into `main` as
`53d9fc9fd`). Every consumer line was read in that tree, and every file:line
in the matrix was confirmed by grep against it. The guard
(`scripts/check_blackboard_contract_matrix.py`) re-checks the consumers on
every run, so a matrix that drifts fails CI rather than quietly describing a
tree that no longer exists.
