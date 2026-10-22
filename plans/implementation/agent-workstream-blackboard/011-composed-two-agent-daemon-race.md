# Agent Workstream Blackboard M011 — The Composed Two-Agent Daemon Race

Status: active — C008 is conditionally closed and supplies the contained
managed-worktree creation this milestone needs. The remaining M005 non-claim
is the composed two-agent daemon race recorded at C006 §9.2; this milestone
discharges it.

## 1. Objective

Prove, in one daemon process, that **two live agents issuing real writes
through the broker against one shared checkout** are serialized by the writer
fence: the two writes never overlap, both land, and neither agent can write
outside the group's checkout.

The claim is about **fence mechanics**, not model quality. The maintainer chose
a **scripted provider** for exactly that reason: the agent's decisions are
deterministic, so the only thing under test is the fence.

This is the last M005 non-claim. M005 closes strictly when this milestone
lands and its evidence is recorded.

## 2. Why this milestone is ready

- **C008 is conditionally closed** (`plans/closure/linux-trusted-git-cost-corrective/008-status.md`).
  It restores the managed-worktree creation path that used to fail closed on
  every host, and it is the prerequisite for the shared checkout being
  allocated at all.
- **The C006 carrier already composes the pieces below the agent.** The
  scheduler, the executor, the coordinator and the fence are composed in
  `tests/shared_checkout_c006.rs`, including a two-member race for one group.
  What it does **not** do is start a child: the pool is built with no agents,
  so the run stops after the placement.
- **The fence is production code.** `src/core/writer_fence.rs`'s
  `DaemonWriterFence` is the daemon's own fence, and
  `DaemonWriterFenceFactory` resolves it from a run's own record. M011 does not
  need a new fence.

## 3. Current implementation evidence

- `tests/shared_checkout_c006.rs:189-244` — the carrier's `Shared::new`
  writes the `managed_worktree` row **straight into SQLite**, because managed
  worktree creation was unavailable in this tree. The comment says so. With
  C008 the row can be produced by the real path.
- `tests/shared_checkout_c006.rs:526-563` —
  `two_members_racing_for_one_group_end_up_on_one_checkout` races two
  `coordinator.acquire` calls and asserts both land on one checkout. This is
  the placement race; it is not the **write** race.
- `src/agent/worker.rs:1183-1187` — a delegated run resolves its agent by
  name: `self.agents.iter().find(|a| a.name == *agent_name)`. A run with no
  matching agent fails with `Agent '<name>' not found`.
- `src/agent/worker.rs:319` — `SubAgentPool` holds
  `provider_registry: Arc<ProviderRegistry>`; the agent's `provider/model` is
  resolved from it.
- `tests/agent_loop_harness.rs:261-421` — the only `ScriptedProvider` in the
  tree, and it is **private to that file**. `build_test_agent_loop` and its
  siblings are private too. A separate integration test cannot use them.
- `src/core/writer_fence.rs:256` — `DaemonWriterFence::begin` is where a
  mutating operation takes its permit.

## 4. Invariants that must not regress

1. **The fence is the authority.** A mutating operation on a shared checkout
   runs only inside a permit. M011 must not weaken the fence to make the race
   pass.
2. **One checkout per group.** Both agents land on the *same* worktree. The
   placement race is already proven; M011 adds the write race.
3. **A run outside the group may not write the shared checkout.** C006
   proves this; M011 must not change it.
4. **The shared profile denies every process-capable tool.** A shared
   checkout refuses process execution; M011's agents must work within that.
5. **No production behaviour changes for any other caller.** M011 adds test
   support and evidence. If it needs a production seam, the seam is narrow and
   is recorded.

## 5. Scope

**In scope**

- A shared, reusable **scripted provider** for integration tests, extracted
  from `tests/agent_loop_harness.rs` into a shared test-support module.
- A composed scenario: one daemon process, one group, one shared checkout
  allocated through the **real** `WorktreeService::create` path, two live
  agents, two real mutating broker operations, one fence.
- The evidence that the two operations never overlap and both land.

**Out of scope**

- Any change to the fence's policy, the permit model, the placement rules or
  the platform gate.
- Any claim about model quality, tool selection or turn behaviour.
- Any Windows evidence. Full Windows support is not a target on this line: the
  Q4 platform criterion is met by the gate **disabling** the unqualified
  platform with a named reason, so no Windows evidence is required and none is
  claimed.
- Any change to the contained Git operation set. ADR-0048 owns that, and
  ADR-0044 remains proposed.

## 6. Required production changes

**None are expected, and that is the point.** M011's production surface is
already correct; what is missing is the composed evidence.

Two seams may need to become reachable from an integration test, and both are
narrow:

- **A shared scripted provider.** Today it exists only inside
  `tests/agent_loop_harness.rs`. It has to move to a module both that file and
  the new test can use. If the shared test-support directory does not already
  exist, create it under `tests/` and keep it test-only: no production crate
  may gain a scripted provider.
- **The pool's agent and provider registration.** A test has to register two
  agents and one provider model with a `SubAgentPool`. If the existing
  constructors cannot express that without a production change, the change is
  limited to making an existing constructor usable.

If the composition turns out to need a *new* production seam — for example a
way for a test to observe the permit timeline — that seam is a
`WriterFencePermitSettler` observation point, it is narrow, and it is
recorded in the closure with the reason it was necessary.

## 7. Ordered work packages

### WP1 — a shared scripted provider

*Intent:* one deterministic provider, usable from more than one integration
test.

Move `ScriptedProvider` and the agent-loop construction helpers out of
`tests/agent_loop_harness.rs` into a shared test-support module. The provider
records the requests it received, which the race assertion needs: it is the
evidence that each agent really ran and really issued its call.

`tests/agent_loop_harness.rs` must keep working unchanged in behaviour.

### WP2 — two live agents in one pool

*Intent:* the pool holds two named agents, and each run resolves its own.

Register two agents with distinct names and the same `provider/model`, so
both resolve through the same `ProviderRegistry` entry. A run that names an
agent the pool does not hold must still fail with the existing typed refusal —
that path is covered by existing tests and must not change.

### WP3 — the shared checkout, allocated for real

*Intent:* the group's checkout comes from `WorktreeService::create`, not from
a hand-written row.

The composed scenario builds the `WorktreeService` the way the daemon builds
it — with the contained creator installed — and allocates the group's checkout
through it. The C006 carrier's hand-written row stays as it is: C006's claim
is about the placement, and changing it would put C006's evidence at risk.

This is where C008 earns its place. On a host that cannot obtain the M004/M005
tiers the contained creation fails closed, and M011's scenario reports that
rather than pretending. See §14.

### WP4 — two real mutating broker operations

*Intent:* the agents write, through the broker, against that one checkout.

Each agent is driven by the scripted provider to issue one **mutating** broker
operation against the shared checkout — a file write, not a read. The
operation must be one the shared profile permits; the shared profile denies
every process-capable tool, and the operation must respect that.

The two operations are issued **concurrently**, from the two agents, inside one
process.

### WP5 — the race assertions

*Intent:* the fence is shown to serialize, not merely to exist.

The scenario asserts:

1. Both agents ran and both issued their operation (the scripted provider's
   recorded requests are the evidence).
2. Both operations landed on the **same** checkout.
3. The two operations **never overlapped** — the permit timeline shows one
   permit released before the other was taken. This is the actual claim.
4. The checkout's final content contains **both** writes.
5. Neither agent wrote outside the group's checkout.

## 8. Failure, cancellation, restart and contention semantics

- The permit is the serialization point. If a writer fails mid-operation, the
  permit is settled in a way that lets the next writer proceed; M011 must not
  leave a permit held across a failure.
- A cancelled run releases its permit.
- The two agents' operations are independent; a failure of one must not make
  the other's result ambiguous.
- If the fixture cannot be raced **fairly** — if one agent always wins
  because of scheduling — the scenario must say so rather than report a
  vacuous pass. The scenario runs the race more than once, and a pass that only
  happens when the agents never actually overlap is a failure, not a success.

## 9. Compatibility and migration

- No schema migration. The worktree record, the permit store and the group
  store are unchanged.
- No wire or protocol change.
- No change to the fence's policy, the platform gate, or the shared
  profile's tool denial.
- The shared test-support module is **test-only**. No production crate gains a
  scripted provider.

## 10. Required tests

1. `two_live_agents_racing_on_one_shared_checkout_never_overlap` — the
   composed scenario. This is the milestone's evidence.
2. `both_agents_wrote_to_the_same_checkout` — the payload assertion, so that
   "never overlapped" cannot be satisfied by two no-ops.
3. `the_composed_race_needs_both_agents_to_actually_run` — the vacuity guard:
   if the agents did not both issue their operation, the scenario fails.
4. `a_run_outside_the_group_still_may_not_write_the_shared_checkout` — the
   C006 invariant, re-asserted in the composed scenario.
5. The existing C006 suite still passes, unchanged.
6. The existing agent-loop harness still passes, unchanged.

## 11. Required verification commands

```bash
cargo fmt --all && cargo fmt --all -- --check
CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p codegg --test shared_checkout_c006
cargo test -p codegg --test <the new composed-race test>
CARGO_BUILD_JOBS=2 bash scripts/verify.sh quick
```

The composed race needs a host that can obtain the M004 process/IPC and M005
offline-network tiers, because the shared checkout is allocated through the
contained Git route. Where the host cannot, the scenario reports the missing
tier and the milestone stays **conditionally closed** — the same disposition
C006 carries.

## 12. Documentation updates

- `architecture/agent_workstream_blackboard.md` (or the blackboard line's
  owning doc) — the composed race, and what M005's closure rests on.
- `architecture/session_launch.md` — if the shared-checkout allocation path
  changes.
- `architecture/git.md` — only if C008's shipped API moves.
- `plans/registry.md` — M011's row and M005's transition.
- `plans/subsystems/agent-workstream-blackboard-roadmap.md` — M011's row.

## 13. Acceptance criteria (externally observable)

1. Two live agents, inside one daemon process, each issued a real mutating
   broker operation against one shared checkout.
2. The two operations never overlapped: the permit timeline shows one
   released before the other taken.
3. Both writes are present in the checkout afterwards.
4. Neither agent wrote outside the group's checkout.
5. The shared checkout was allocated through the production path, not written
   into the database by hand.
6. A run outside the group still cannot write the shared checkout.

## 14. Stop conditions

The agent **reports rather than improvises** if any of these occur:

- The composition cannot be built without weakening the fence, the placement
  rules, or the shared profile's tool denial. **Stop.**
- The shared checkout cannot be allocated on this host. **Stop** and report
  which tier was missing. Do not fall back to the hand-written row: that is
  C006's fixture, and reusing it would make the milestone's claim vacuous.
- The race turns out not to be schedulable — the two operations never
  actually overlap, so the serialization is not being tested. **Stop** and
  report it.
- The scripted provider turns out to be insufficient to drive a real broker
  operation. **Stop**; do not add a production-only test hook that a model
  could reach.
- Composing the race would require changing the writer fence's policy or the
  platform gate. **Stop** and propose a separate decision.

## 15. Closure evidence required

- The closure record at
  `plans/closure/agent-workstream-blackboard/011-status.md`, with the
  required sections.
- The requirement-to-evidence matrix mapping each acceptance criterion to a
  named test and its result, with local and hosted evidence labelled
  separately.
- The **permit timeline** — the actual evidence that the two operations did
  not overlap, not a summary of it.
- The scripted provider's recorded requests — the evidence that both agents
  really ran.
- The non-regression evidence that C006's suite still passes unchanged.
- An explicit statement of what is **not** claimed: no claim about model
  quality, no Windows evidence, no claim about any other Git operation, and no
  claim that the shared profile permits process execution.
- The registry reconciliation, including the unblock audit: this closure
  unblocks **M005's strict closure**, which must be registered in the same
  commit.

## 16. Handoff notes

- The authority is **ADR-0048** for the contained Git route and
  **ADR-0047** for the shared-checkout profile. Neither is widened here.
- ADR-0044 stays proposed.
- The **scripted provider** is test-only and must never become a production
  capability a model can reach.
- The composed race is about **fence mechanics**. A pass that comes from the
  two agents never overlapping is not a pass.
- Windows produces no evidence here and claims none.
