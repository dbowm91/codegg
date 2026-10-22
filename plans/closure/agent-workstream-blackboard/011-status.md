# M011 — The Composed Two-Agent Daemon Race — closure record

**Status:** closed
**Milestone:** M011 — The Composed Two-Agent Daemon Race
**Plan:** `plans/implementation/agent-workstream-blackboard/011-composed-two-agent-daemon-race.md`
**Roadmap:** `plans/subsystems/agent-workstream-blackboard-roadmap.md` (M011)
**Closed:** 2026-10-22
**Branch:** `work/linux-trusted-git-c008-contained-worktree`
**Unblocks:** **M005** — the agent-workstream blackboard's strict closure.

## 1. What this milestone is

M005 claimed that two bounded agents sharing **one** checkout cannot write it
at the same time. C006 proved the *mechanism* — the writer permit refuses a
second writer while the first holds it — but it drove the fence **directly**.
It never started an agent.

M011 composes the whole thing: one process, one group, one shared checkout
allocated through the **real** `WorktreeService::create` path, two **live**
agents, two **real** mutating broker operations, and the **production** fence.

It is the last M005 non-claim. M005 closes strictly on this record.

## 2. The finding: the shared-checkout writer could not work at all

The composed scenario could not complete a single write before this was fixed,
and the reason is worth stating plainly because it is a **production defect,
not a test artifact**.

Every mutating tool takes its writer permit under an idempotency key built by
`writer_fence_key` (`src/tool/writer_fence.rs`). That key was the readable
form:

```text
{tool}:{target paths}:{invocation key}
```

The permit store bounds an idempotency key at **128 bytes**
(`MAX_WRITER_PERMIT_IDEMPOTENCY_BYTES`), and the bound is also a `CHECK`
constraint on `writer_permit.idempotency_key`. A managed checkout path alone
runs past a hundred bytes, and the invocation key
(`agent-invocation-<sha256>`, eighty bytes) is another eighty. The readable key
was therefore **always** over the bound, for every tool and every checkout.

The store refused it, and the refusal reached the model as:

```text
the checkout writer fence is unavailable: writer permit scope is invalid
```

So on a shared checkout, `write`, `edit`, `replace`, `apply_patch`, `commit`
and the typed `git` mutations were **all** refused. M005's shared writer was
unreachable in production.

**Why C006 did not find it.** C006 drove `DaemonWriterFence::begin` directly
with short hand-made keys (`"c006-first"`, `"c006-second"`). Those fit. Only
a composed run through the real tool — where the key is built from the real
path and the real invocation key — hits the bound. This is precisely the
difference the milestone was written to expose: a suite that proves a
mechanism is not the same as a suite that proves the mechanism is *used*.

## 3. What changed

### 3.1 The fix — a bounded, digested idempotency key

`writer_fence_key` now digests its material:

```text
wf1:{tool label}:{sha256 hex}
```

The properties the permit store relies on are preserved:

- **Stable for a retry.** The same operation retried carries the same key, so
  the store recognises it as the same acquisition and not a conflict.
- **Distinct for two different operations.** The tool name, the target paths
  and the invocation key are all in the digest's material, so two writes never
  collide.
- **Bounded by construction.** The label is capped at 32 characters and the
  digest is 64 hex characters, so the key is at most 101 bytes — under the
  store's bound whatever the path and the invocation key are.

The change is one function plus three unit tests beside it
(`src/tool/writer_fence.rs`'s `tests` module). **No fence policy, no permit
model, no placement rule and no platform gate changed.** The key is an
identity, not something a human reads; the readable prefix keeps the tool
name visible in the permit row.

### 3.2 The shared test support

Two modules, both **test-only**, both in `tests/common/`:

- `scripted_provider.rs` — the `ScriptedProvider`, moved out of
  `tests/agent_loop_harness.rs` so two suites can use it, plus two additions
  the race needs: an optional **barrier** that holds a provider's *first*
  request until every participant has arrived, and a record of **when** each
  request arrived.
- `linked_worktree.rs` — `LinkedWorktreeCreator`, which produces a **real
  linked worktree** by the same two Git operations the contained child runs:
  a detached `git worktree add`, then `git symbolic-ref HEAD` to attach the
  branch. C008's suite now uses the same creator, so the tree has one
  implementation of "a real linked worktree".

`tests/agent_loop_harness.rs` includes the provider module by `#[path]`
rather than through `mod common;`, so its binary's test composition — and with
it the scheduling of tests that share the global event bus — is unchanged.

### 3.3 The composed scenario

`tests/composed_two_agent_race_m011.rs`. Three tests, described in §5.

## 4. What the scenario is, precisely

- **One process.** One SQLite pool, one worktree service, one group service,
  one permit service.
- **One group, one shared checkout.** The group's checkout is allocated by
  `SharedCheckoutCoordinator::acquire` → `WorktreeService::create` →
  `create_reserved_contained`, which is the **production** path. It is never a
  hand-written `managed_worktree` row — C006's carrier does that, and reusing
  it would make the milestone's claim vacuous.
- **A real linked worktree.** The service's own post-create validation is not
  relaxed: `git worktree list` must report the checkout at the recorded path
  **on the recorded branch**, and `is_git_worktree` must accept it.
- **Two live agents.** Two `AgentLoop`s run in one process, each carrying the
  same two named agents (`alpha`, `beta`) and its own scripted provider, and
  each selecting the agent it runs. Both names resolve in both loops and each
  run resolves its own.
- **Two real mutating broker operations.** The production `write` tool, through
  the production broker, into the shared checkout — not a bespoke test tool.
- **The production fence.** `DaemonWriterFence`, one per run, resolved from
  that run's durable record, over the production `WriterPermitService`.

### 4.1 The creator that ran, stated plainly

The fixture installs `LinkedWorktreeCreator`. It is **not** the production
contained creator and it claims **no containment**: it runs Git directly, with
no sandbox and therefore no network enforcement of its own. The containment
contract is C008's, proved by C008's own suite and by the hosted M004 lane.

The milestone's subject is the fence, and the fence claim does not depend on
how the checkout was made. What the creator must not be is a hand-written
database row, and it is not.

### 4.2 The composition is at the agent-loop level, not through a `SubAgentPool`

This is a departure from the plan's WP2 wording, and the reasons are
structural:

- An `AgentLoop` carries **one** provider. Two agents in one loop would share
  one scripted response sequence, so the two turns could not be driven
  independently.
- `AgentLoop::run` takes `&mut self`, so two concurrent turns cannot share one
  loop even with two providers.
- A `SubAgentPool` reaches the same fence through a delegated child's own loop
  and its own durable-run plumbing. That path is covered by the worker and by
  C006's carrier; the milestone's claim is about the fence.

What is **not** claimed by this composition: the delegated child's own
transport and durable-record plumbing.

## 5. Requirement-to-evidence matrix

Local evidence is from this workstation (Linux 6.8, Git 2.43.0). Hosted
evidence is labelled separately; the M011 suite is host-independent, so the
hosted column is the repository's own CI lane on the merged head.

| # | Acceptance criterion | Test | Local | Hosted |
|---|---|---|---|---|
| 1 | Two live agents in one process each issue a real mutating broker operation against one shared checkout | `two_live_agents_racing_on_one_shared_checkout_never_overlap` | **pass** | CI lane |
| 2 | Both agents really ran and really issued their operation | same test (the provider's recorded requests and instants) | **pass** | CI lane |
| 3 | The race was **fair**: the two tool-call windows overlapped, so the pass is not an artifact of a sequential run | same test (the windows are asserted to overlap) | **pass** | CI lane |
| 4 | The two operations never overlapped: the permit timeline shows one permit released before the other was taken | same test (the four permit events, in order) | **pass** | CI lane |
| 5 | Both writes landed on the same checkout | same test (both files read back) | **pass** | CI lane |
| 6 | The permit store agrees: the checkout's permit is free again and went through the whole state machine | same test (the `writer_permit` row) | **pass** | CI lane |
| 7 | A run outside the group may not write the shared checkout | `a_run_outside_the_group_may_not_write_the_shared_checkout` | **pass** | CI lane |
| 8 | The shared checkout is a real linked worktree that Git reports on the recorded branch | `the_shared_checkout_is_a_real_linked_worktree` | **pass** | CI lane |
| 9 | The fence's idempotency key fits the permit store's bound, is stable for a retry and is distinct for two operations | `src/tool/writer_fence.rs`'s three unit tests | **pass** | CI lane |
| 10 | The C006 carrier still passes, unchanged | `cargo test -p codegg --test shared_checkout_c006` | **pass** | CI lane |
| 11 | The agent-loop harness still passes, unchanged | `cargo test -p codegg --test agent_loop_harness` | **pass** | CI lane |

## 6. The permit timeline — the actual evidence

The fence reports every permit transition through `tracing`, and the scenario
reads those reports rather than asking the fence to behave differently for a
test. The four events, in the order the daemon emitted them:

```text
the checkout writer fence took the checkout's writer permit
    agent_run_id=<the first writer>   worktree_id=<the shared checkout>
the checkout writer fence released the checkout's writer permit
    agent_run_id=<the first writer>   worktree_id=<the shared checkout>
the checkout writer fence took the checkout's writer permit
    agent_run_id=<the second writer>  worktree_id=<the shared checkout>
the checkout writer fence released the checkout's writer permit
    agent_run_id=<the second writer>  worktree_id=<the shared checkout>
```

The scenario asserts the four events are exactly these: an acquire and a
release for one run, then an acquire and a release for the other, with the
first run's release **before** the second run's acquire. That is the claim —
*one permit released before the other taken* — stated as an ordered fact
rather than a summary.

The durable row agrees: after both writers are done, `writer_permit` reads
`state = 'available'`, `owner_agent_run_id IS NULL`, and a revision of at
least 6 — each writer's acquisition and release is three state changes, so two
writers are six.

### 6.1 Why the ordering is stable

The scenario runs on a `current_thread` runtime. The permit store's
compare-and-swap is what serializes the writers, and the release path
notifies the waiting writer **after** the row has been freed and **before** the
fence logs the release — so on a single-threaded runtime the release report is
always emitted before the other writer's acquisition report. The race was run
**twenty times** in a row locally, and every run produced this order.

## 7. The fairness guard — the vacuity guard, observed

The two providers hold their **first** request at a barrier until both have
arrived. Neither agent can finish its turn before the other has started one.

That is a construction guarantee, so the suite also *observes* it rather than
trusting it. The provider records when each request arrived: the first
request opens the agent's tool-call window and the second closes it. The
scenario asserts the two windows **overlap**:

```text
max(first request of alpha, first request of beta)
  <
min(second request of alpha, second request of beta)
```

A pass that came from the two agents running one after the other therefore
**fails** the scenario instead of satisfying it.

## 8. The outsider invariant, re-asserted in the composed scenario

The outsider holds a checkout of its own — leased by a **different** run — so
its fence resolves against a lease it does not hold. The scope it was handed
names the shared checkout, and the run's own durable record wins over that
scope: a caller cannot borrow another checkout's authority by naming it.

The scenario asserts the outsider's write produced **no file**, and that the
shared checkout's permit timeline is **empty** — the outsider never took a
permit either.

## 9. Invariant review

- **The fence is still the authority.** The scenario uses the production
  `DaemonWriterFence`, the production permit service and the production
  `write` tool. Nothing was weakened to make the race pass.
- **The permit's semantics are unchanged.** The fix changes the *shape* of an
  idempotency key, not the permit model, the lease generation binding, the
  capacity bound, or the quarantine rules.
- **One checkout per group.** Both agents are placed on the same worktree
  through the coordinator; the placement race remains C006's claim.
- **The shared profile still denies process-capable tools.** The scenario
  writes files and spawns nothing.
- **No schema migration, no wire change, no new event variant, no new guard
  script.** The `writer_permit` table is untouched.
- **The scripted provider is test-only.** No production crate gained one, and
  a model cannot reach it.

## 10. Stop conditions

The plan's stop conditions, and what happened to each:

- *"The composition cannot be built without weakening the fence, the placement
  rules, or the shared profile's tool denial. **Stop**."* — **Did not fire.**
  The fence, the placement rules and the tool denial are untouched.
- *"The shared checkout cannot be allocated through the production contained
  creator on this host. **Stop** and report which tier was missing … the
  milestone may still be composed with a fixture creator that produces the same
  real linked worktree, provided the closure says plainly which creator ran."* —
  **Fired, and was answered as the plan allows.** This host cannot obtain the
  M004 process/IPC tier (Landlock ABI 4, needs ABI 6), so the fixture creator
  ran. §4.1 says so plainly, and the same creator is shared with C008's suite.
- *"The race turns out not to be schedulable — the two operations never
  actually overlap. **Stop** and report it."* — **Did not fire.** The two
  tool-call windows are asserted to overlap, and the permit timeline shows the
  two operations serialized. See §6 and §7.
- *"The scripted provider turns out to be insufficient to drive a real broker
  operation. **Stop**; do not add a production-only test hook that a model
  could reach."* — **Did not fire**, and no test hook was added. The provider
  drove the production `write` tool through the production broker.
- *"Composing the race would require changing the writer fence's policy or the
  platform gate. **Stop** and propose a separate decision."* — **Did not fire.**
  The fence's *policy* did not change. Its idempotency-key *encoding* was
  defective, and the fix is recorded in §2 and §3.1.

The one thing the plan did not anticipate is §2: a production defect that made
the milestone's subject unusable. The plan's §6 said "**None are expected, and
that is the point**", and that expectation was wrong. The correction is
recorded here rather than smoothed over.

## 11. Verification executed

Local, this workstation:

```
cargo fmt --all -- --check                                              pass
cargo clippy --workspace --all-targets --locked -- -D warnings          clean
cargo test -p codegg --lib tool::writer_fence                            3 passed, 0 failed
cargo test -p codegg --test composed_two_agent_race_m011                  3 passed, 0 failed
cargo test -p codegg --test composed_two_agent_race_m011   (× 20 runs)  20 passed, 0 failed
cargo test -p codegg --test shared_checkout_c006                          unchanged, passes
cargo test -p codegg --test agent_loop_harness                            unchanged, passes
cargo test -p codegg --test contained_worktree_c008                       5 passed, 0 failed
bash scripts/verify.sh quick                                            "Quick verification passed."
```

The twenty-run repetition is the stability evidence for the permit ordering
(§6.1). It is a command, not a test: the scenario itself asserts the order.

**A pre-existing failure, not caused by this milestone.**
`tests/agent_loop_harness.rs`'s `temp_root_write_following_the_offer_needs_no_human`
fails under `cargo test`'s in-process, multi-threaded runner, on the
**pristine** tree as well as this one — it counts `PermissionPending` events
on the **global** event bus, so a concurrent test's permission request lands
in its count. It passes serially (`--test-threads=1`, 72 passed) and it passes
under `cargo nextest`, whose `ci` profile runs one process per test — which is
what CI uses. It is recorded here because it is a real latent flake in that
suite, not because this milestone caused it.

## 12. Non-claims

- **No claim about model quality, tool selection or turn behaviour.** The
  model is scripted; the only thing under test is the fence.
- **No Windows evidence, and none is claimed.** Full Windows support is not a
  target on this line; the platform gate disables Windows with a named reason,
  which is what discharges M005's Q4.
- **No claim about containment.** The checkout was made by a fixture that runs
  Git directly. C008 owns the containment contract.
- **No claim that the two agents can *merge* their work.** Integration remains
  an explicit, typed step that CodeGG never performs on its own.
- **No claim about the delegated child's transport.** The composition is at
  the agent-loop level (§4.2).
- **No claim that an external editor's writes to the same directory are
  mediated.** The contract covers CodeGG's own writers.
- **No claim that the idempotency-key fix changes what a permit *means*.** It
  changes the encoding of a key, and nothing else.

## 13. The unblock audit

M011 discharges M005's **last** non-claim — the composed two-agent daemon race
recorded at C006 §9.2 — and therefore unblocks **M005's strict closure**.
M005's row moves to closed in the same commit as this record, with the
evidence matrix that rests on this one.

Nothing else became newly eligible. C005's parity/benchmark/admission scope is
untouched and stays blocked on its own named blockers.
