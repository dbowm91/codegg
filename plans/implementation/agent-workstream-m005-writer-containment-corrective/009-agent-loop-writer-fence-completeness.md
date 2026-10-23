# M005 Writer-Containment Corrective C009 — Agent-Loop Writer Fence Completeness

Status: closed — C009 closed (`plans/closure/agent-workstream-m005-writer-containment-corrective/009-status.md`). The commit tool takes the fail-closed `None` arm; the daemon's team-policy principal fallback is derived from durable team state at fence time; and the nested-subagent dispatch forwards the parent's fence. One named item remains open in the closure record: a delegated child's own transport principal is unbound, so its write on a team daemon is refused — the fail-closed direction — rather than attributed to a live principal.
Review baseline: `f1b83a7a7` (`origin/main`, 2026-10-23) — the tree §3's evidence was gathered against
Implementation baseline: `f2639b5d2` (`origin/main`, 2026-10-23) — the tree this work is applied to
Source roadmap: `plans/subsystems/agent-workstream-blackboard-m005-writer-containment-corrective-addendum.md`
Long-term requirements: `plans/000-long-term-specification.md` §§9, 16–17, 26–29; `plans/001-terminology-and-domain-model.md` §§5, 7–8
Applicable ADRs: ADR-0002 (agent run worktree isolation and control, accepted), ADR-0032 (agent workstream coordination and shared checkout ownership, accepted), ADR-0047 (restricted agent shared checkout writer contract, accepted option B)
Primary class: invariant

## 1. Objective

Make one claim true end to end:

> **Every model-facing writer that the agent loop offers runs behind a fence that can take the checkout's writer permit, and every write on a shared checkout is attributed to a principal that the authority can distinguish.**

This milestone closes three defects found in the 2026-10-23 implementation review of the blackboard line. All three are minors, and all three break the claim in the same way: a path exists on which a writer runs with **no fence**, or with a fence that **cannot take a permit**, or with a **principal the authority cannot distinguish**.

The claim is about the **agent-loop fence**, which C003 interposed on the native file and typed Git writers. The mediation gate is C008's subject and the shared checkout's placement is C007's.

## 2. Why this milestone is ready

- **C003 is closed.** `plans/closure/agent-workstream-m005-writer-containment-corrective/003-status.md` interposed the fence on the native file and typed Git writers. This milestone closes the gaps C003 left in that interposition.
- **The review's agent-loop audit returned PASS** on the six areas it covered — fence reachability, permit lifecycle, path policy, principal resolution, per-call scope, and key determinism. The three defects below are the ones that audit found *outside* those six areas, plus the one it found inside area 4.
- **The permit store's foundations are proven.** `codegg-core --test writer_permit` (21 tests) covers idempotent retries, concurrent acquirers, a panicking holder, unproven quarantine and operator release. This milestone does not reopen them.
- **No new architecture is required.** Every fix is a change to an existing decision point inside the C003 contract. No ADR is needed.

## 3. Current implementation evidence

### 3.1 The `commit` tool is the only mutating tool registered without the fail-closed wrapper (minor)

`ToolRegistry::with_options` is the registry's builder. Its stated rule, in its own comment at `src/tool/mod.rs:718-719`, is that the `None` arms **refuse rather than defaulting**. Eleven workspace tools honour it:

| Tool | `None` arm |
|---|---|
| `read` | `src/tool/mod.rs:725` |
| `edit` | `:732` |
| `write` | `:739` |
| `glob` | `:746` |
| `grep` | `:753` |
| `list` | `:760` |
| `apply_patch` | `:921` |
| `replace` | `:934` |
| `terminal` | `:947` |
| `git` | `:1060` |
| `git_read` | `:1073` |

The commit tool does not:

```rust
let commit_tool = if let Some(root) = workspace_root.as_ref() {
    crate::tool::commit::CommitTool::new().with_workdir(root.clone())
} else {
    crate::tool::commit::CommitTool::new()
};
…
registry.register(commit_tool);
```

(`src/tool/mod.rs:1199-1216`.)

**Consequence.** With `workspace_root: None` the commit tool stays in the registry and is offered to the model. Its workdir is the sentinel `\0codegg:no-workspace-root` (`src/tool/commit.rs:38-41`). `canonicalize_existing_prefix` resolves that to `<checkout_root>/\0codegg:no-workspace-root` (`src/tool/writer_fence.rs:448-475`), which is **lexically under** the root, so the fence's containment check **passes** (`src/tool/writer_fence.rs:418-419`). The tool then acquires the checkout's writer permit, fails with a raw Git error, and reports `Unproven` (`src/tool/commit.rs:404-412`) — which **parks (quarantines) the checkout** rather than releasing it.

**Today's exposure.** The daemon always builds its registry with a root (`src/tool/factory.rs:227`), so the observable defect is a live-but-non-functional mutating tool in the catalogue. The quarantine path needs a rootless registry paired with a managed checkout.

### 3.2 The daemon's team-policy refusal is unreachable (minor)

`DaemonWriterFenceFactory::new` takes a `local_principal_fallback` flag. Its contract, documented at `src/core/writer_fence.rs:487-493`, is:

> A single-user daemon owns its own checkout and names its own principal. A daemon with a team policy leaves this off, so a write that arrives without a bound actor is refused rather than written anonymously.

The only production construction site passes the flag unconditionally:

```rust
crate::core::writer_fence::DaemonWriterFenceFactory::new(
    permit.clone(),
    worktree_service.clone(),
    deps.agent_run_store.clone(),
    Some(deps.run_group_service.clone()),
    true,
)
```

(`src/core/daemon_construct.rs:540-546`.)

The daemon **does** have a team model (`crates/codegg-core/src/team`, `src/core/daemon_team.rs`), but the flag never consults it. So the "shared checkout has no bound actor" refusal at `src/core/writer_fence.rs:346-352` is **dead code** in the daemon.

**Consequence.** Combined with the agent loop never binding an actor — `execution_audit: None` at `src/agent/tool_batch.rs:531`, and the only producer (`src/tool/broker.rs:1492-1494`) is fed from that same `None` — every shared-checkout write is attributed to the synthetic `AuthenticatedPrincipal::local_owner("codegg-writer-fence")`. The permit row cannot distinguish two writers.

**Why this is a latent hazard rather than a present one.** The permit authority is built **without** an `AuthorizationService` (`src/core/runtime_deps.rs:475-479` — there is no `.with_authorization(...)`), so the `Capability::FileModify` check at `src/core/writer_permit.rs:759-775` is skipped. But `local_owner` is authorised under `PolicyKind::LocalOwnerBroad` for **any** capability on **any** project (`crates/codegg-core/src/authorization.rs:639-654`). The moment that seam is wired, the fallback would authorise a team member's write **as the local owner**.

### 3.3 The nested-subagent dispatch drops the fence (minor)

The `task` tool's in-process spawner branch (`src/tool/task.rs:2194-2233`) builds its request with `writer_fence: None` (`src/tool/task.rs:2229`). The worker substitutes a fallback:

```rust
let writer_fence: Option<crate::tool::writer_fence::SharedWriterFence> =
    request.writer_fence.clone().or_else(|| {
        request.workspace_root.as_ref().map(|workspace_root|
            crate::tool::writer_fence::WorkspaceWriterFence::new(workspace_root.clone())
        )
    });
```

(`src/agent/worker.rs:1562-1567`.)

`WorkspaceWriterFence` is a **containment-only** fence: its `begin` returns `Ok(None)` and can **never** take a permit (`src/tool/writer_fence.rs:511-519`).

**Consequence.** The nested child inherits the parent's workspace root (`src/tool/task.rs:2218`), which for a delegated run **is** the managed checkout (`src/scheduler/executors.rs:1139`, `:1191`, `:1342`). A grandchild that writes would therefore write into the shared checkout **without taking the checkout's writer permit** — so it can overlap with the group sibling that does hold the permit. The mutual exclusion the permit exists to provide is lost for that writer.

The worker's own comment states the fallback's premise — *"the containment-only fence below is the fallback for a run that holds no managed checkout"* — which is false for this case: the child executes **inside** one.

**Today's exposure.** The shipped daemon does not take this path: `configure_durable_delegation` runs at startup before the pool serves (`src/core/daemon_construct.rs:798-805`), and the durable branch then routes the nested child through the scheduler, whose executor resolves the fence from the child's durable run record (`src/scheduler/executors.rs:1282-1304`, `:1361`). The hole is one configuration change away — durable delegation not configured, or `configure_durable_delegation`'s `try_lock` losing the race (`src/agent/worker.rs:706-708`), which silently leaves the slot `None` forever.

## 4. Invariants that must not regress

1. **A mutating tool with no workspace root refuses.** The `None` arms refuse rather than defaulting (`src/tool/mod.rs:718-719`). §3.1 extends this to the commit tool; the other eleven must not change.
2. **A missing workspace path policy is refused** (`src/agent/tool_batch.rs:1928-1933`) and a mismatched one is refused (`src/tool/broker.rs:397-404`).
3. **An unattributed write is refused on a shared checkout** unless the daemon is a single-user daemon with no team policy (`src/core/writer_fence.rs:346-352`).
4. **The group dimension is the primary gate.** A run outside the group is refused at `src/core/writer_fence.rs:285-293` and independently by the authority's `NotLeaseOwner` (`src/core/writer_permit.rs:726-739`). The principal is not the gate.
5. **Every mutating tool acquires before its effect and settles on every exit path**; a settle failure leaves the guard unsettled and its `Drop` quarantines rather than freeing (`src/core/writer_permit.rs:575-605`). Mutating tools are `DirectOnly` + `NonIdempotent` with `max_retries: 0` (`src/tool/contract.rs:264-281`), so neither the broker nor the batch loop re-runs them.
6. **The permit is per-tool-call, not per-batch**, matching the single-owner semantics (`src/agent/tool_batch.rs:1887-1890`).
7. **The idempotency key is deterministic** — a SHA-256 over `agent-invocation-v2/{owner_scope}/provider-turn/{n}/tool-call/{id}/ordinal/{n}` (`src/agent/tool_batch.rs:2531-2554`), with no map iteration order, timestamp or random value — over a store holding at most one row per `worktree_id` (`crates/codegg-core/src/writer_permit.rs:707`, `:765-792`).
8. **The group's share is derived from durable state, never asserted by the caller** (`src/core/writer_fence.rs:295-297`).

## 5. Scope

**In scope**

- The `commit` tool's registration arm in `src/tool/mod.rs`.
- The daemon's `local_principal_fallback` determination, and the transport principal's binding on a team daemon.
- The nested-subagent dispatch's fence forwarding, and the refusal that replaces the silent downgrade.
- Regression tests for each defect, and the guards in §10.

**Out of scope**

- The fence authority chain's ordering and the permit store's state machine — both reviewed and correct.
- The mediation gate and the reservation authority — C008's subject.
- The shared checkout's placement and lease lifetime — C007's subject.
- Any change to the tool registry's *shape*, the capability table, or the eleven `None` arms that already refuse.
- The `AuthorizationService` seam itself. §3.2's fix must make the team-policy branch reachable and correct **without** wiring that seam; wiring it is separate work with its own plan.
- The `codegg exec` / `codegg --run` rootless-registry observation. That is pre-existing on `main` (`590c713ea`), outside this line, and is recorded here only as an observation.

## 6. Required production changes

### 6.1 Register the commit tool behind the fail-closed wrapper

`src/tool/mod.rs:1199-1216` must take the same `None => unrooted(Box::new(...))` arm as the other eleven workspace tools, and must register through `register_boxed` so the wrapper survives.

The root-present arm keeps `CommitTool::new().with_workdir(root.clone())` unchanged.

### 6.2 Derive the principal fallback from the daemon's team state, and bind the transport principal

Two changes, both required:

1. **`local_principal_fallback` must reflect the daemon's team policy.** A daemon with a team policy must not install a local-principal fallback; a single-user daemon with no team policy keeps one. The determination belongs where the daemon already knows its team state (`src/core/daemon_construct.rs:545`), not as a literal.
2. **A team daemon must bind the transport principal** on its writes, so the permit row can distinguish writers and the "unattributed write" refusal is reachable rather than dead.

The second change means the agent loop's execution context must carry a real principal rather than `execution_audit: None` (`src/agent/tool_batch.rs:531`). The principal must come from the daemon's trusted transport — not from the model, and not from anything the model can influence.

**Constraint.** The fallback's purpose is to let a *single-user* daemon attribute its own writes. It must not be removed outright, or the single-user daemon's own writes become unattributed and refused — that would be a regression, not a fix.

**Explicitly not in scope:** wiring the `AuthorizationService` into the permit authority (`src/core/runtime_deps.rs:475-479`). This milestone makes the branch reachable and correct; it does not enable the `Capability::FileModify` check.

### 6.3 Forward the fence into the nested-subagent dispatch, or refuse the dispatch

The `task` tool's in-process spawner branch must either carry the parent's fence into the `SubAgentRequest`, or refuse the nested dispatch when the parent's checkout is a managed or shared one and no fence can be resolved.

The preferred shape is to forward the fence: `TaskTool` currently has no `writer_fence` field (`src/tool/task.rs:386-400`), and adding one that is threaded from the loop's services gives the nested child the parent's fence — which can take the permit.

The refusal is the fallback for the case where no fence is available at all. It must be a typed refusal, and it must not be silently downgraded to a containment-only fence.

**The worker's fallback itself must be corrected.** `src/agent/worker.rs:1562-1567` substitutes a containment-only fence when the request carries none. That substitution is only correct for a run that holds **no managed checkout**. When the request's workspace root *is* a managed checkout, the fallback must not silently downgrade; the run must either receive a real fence or be refused.

## 7. Ordered work packages

### WP1 — Register the commit tool behind `unrooted`

**Intent:** no mutating tool survives the loss of the workspace root.

**Required changes:** §6.1.

**Acceptance evidence:** a registry built with `workspace_root: None` refuses the commit tool the way it refuses the other eleven; the commit tool's sentinel workdir never reaches the fence's containment check; a failed commit cannot park a checkout from a rootless registry.

### WP2 — Derive the principal fallback and bind the transport principal

**Intent:** a shared checkout's writers are distinguishable, and the unattributed-write refusal is reachable.

**Required changes:** §6.2.

**Acceptance evidence:** a daemon with a team policy installs no local-principal fallback and refuses an unattributed shared-checkout write; a single-user daemon still attributes its own writes and is not refused; two writers on one shared checkout produce two distinguishable principals in the audit trail.

### WP3 — Forward the fence into the nested dispatch

**Intent:** a nested child cannot write into the parent's managed checkout without the permit.

**Required changes:** §6.3.

**Acceptance evidence:** a nested child dispatched from a run on a managed checkout either carries a fence that can take the permit, or is refused with a typed error; the worker's containment-only fallback is not used for a run that holds a managed checkout.

### WP4 — Guards and regression suite

**Intent:** the defects cannot recur silently.

**Required changes:** §10.

**Acceptance evidence:** every guard is wired into `scripts/verify.sh quick` or CI, and each has a `--self-test` case that must be rejected.

## 8. Failure, cancellation, restart and contention semantics

- **The daemon has no team policy.** The local-principal fallback stays installed and the daemon's own writes remain attributable. The single-user path must not regress into refusing itself.
- **The daemon has a team policy but a write arrives without an actor.** The write is refused with the typed "no bound actor" error, not written anonymously.
- **A permit settles and the settle fails.** The guard stays unsettled and its `Drop` quarantines the checkout (`src/core/writer_permit.rs:575-605`). This is unchanged by this milestone and must stay the behaviour.
- **The nested dispatch finds no fence.** The dispatch is refused; it must not fall back to a containment-only fence on a managed checkout.
- **The daemon restarts.** The principal binding is derived from durable daemon state and must survive the restart the same way the group membership does.
- **Two writers contend.** Unchanged: the permit's CAS on revision + lease generation + owner token + state admits exactly one holder.

## 9. Compatibility and migration

- **No schema change is expected.** §6.2 changes which principal a write is attributed to, not the shape of a permit row. `writer_permit.owner.principal_id` already carries it.
- **The `commit` tool's registration change** alters the tool catalogue only for a registry built with `workspace_root: None`. A rootless registry already cannot use the tool — the sentinel workdir makes it fail — so this converts a confusing failure into a clean refusal. No user-visible capability is removed.
- **The principal binding** changes what the audit trail records for shared-checkout writes. The recorded principal moves from a synthetic local-owner value to the transport principal. This is a diagnostic change with no stored-record migration.
- **The nested-dispatch refusal** can turn a previously-running nested child into a refused one in the configurations where the fence is unavailable. That configuration is not the shipped default (§3.3), and the refusal is the intended correction.

## 10. Required tests

**Focused unit tests**

- `src/tool/mod.rs` — a registry built with `workspace_root: None` refuses every mutating tool, including `commit`.
- `src/core/writer_fence.rs` — the "no bound actor" refusal is reachable when the fallback is absent.
- `src/agent/worker.rs` — a request whose workspace root is a managed checkout and which carries no fence is refused rather than downgraded.

**Integration tests**

1. `a_rootless_registry_refuses_every_mutating_tool` — including `commit`, which is the regression this milestone fixes.
2. `a_team_daemon_refuses_an_unattributed_shared_checkout_write` — the branch that is dead code today.
3. `a_single_user_daemon_still_attributes_its_own_writes` — the non-regression for §6.2.
4. `two_writers_on_one_shared_checkout_have_distinguishable_principals`.
5. `a_nested_child_on_a_managed_checkout_takes_the_permit_or_is_refused` — and its counterpart, `a_nested_child_with_no_fence_is_not_downgraded_to_containment_only`.

**Static guards** (new, each with a `--self-test`)

- A guard asserting every mutating tool in the registry has a `None` arm that refuses — that no mutating tool is registered directly from a `Default`-shaped constructor.
- A guard asserting `local_principal_fallback` is not a literal `true` at its production construction site.
- A guard asserting the worker's containment-only fallback is not used when the request's workspace root is a managed checkout.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=2 bash scripts/verify.sh quick
CARGO_BUILD_JOBS=2 cargo test --locked -p codegg --lib tool::writer_fence
CARGO_BUILD_JOBS=2 cargo test --locked -p codegg-core --test writer_permit
CARGO_BUILD_JOBS=2 cargo test --locked -p codegg --test shared_checkout_c006
CARGO_BUILD_JOBS=2 cargo test --locked -p codegg --test composed_two_agent_race_m011
python3 scripts/check_writer_disposition.py --self-test
```

Never run `--all-features` for a workspace sweep.

## 12. Documentation updates

- `plans/subsystems/agent-workstream-blackboard-m005-writer-containment-corrective-addendum.md` — the milestone table gains C009; the status line records the open corrective work.
- `architecture/writer_permit.md` — the documented principal attribution must describe the transport principal, the team-policy refusal, and the single-user fallback's remaining role.
- `architecture/blackboard_contract_matrix.md` — if the matrix names the writer's principal, it must name the transport principal.
- `docs/security-semantics.md` — if it describes how a shared-checkout write is attributed, the attribution change must be recorded.
- `docs/tools.md` — the `commit` tool's behaviour with no workspace root, if the document describes it.

## 13. Acceptance criteria

1. A registry built with `workspace_root: None` refuses **every** mutating tool, including `commit`.
2. A daemon with a team policy installs no local-principal fallback, refuses an unattributed shared-checkout write, and binds the transport principal so two writers on one shared checkout are distinguishable.
3. A single-user daemon still attributes its own writes and is not refused by its own fence.
4. A nested child dispatched from a run on a managed checkout takes the checkout's writer permit or is refused with a typed error — never silently downgraded to containment-only.
5. Every defect above has a regression test that fails against the baseline and passes after the fix, and every new guard is wired in with a `--self-test` case that is rejected when the defect is reintroduced.
6. `verify.sh quick`, `cargo clippy --workspace --all-targets -- -D warnings` and the named test targets are green.

## 14. Stop conditions

Report rather than improvise if:

- **§6.2's transport principal is not available** on a path where a write reaches the fence. Report the path; do not fall back to a synthetic principal, which is the defect.
- **Binding the transport principal requires the `AuthorizationService` seam** to be wired. That seam is explicitly out of scope — report it and stop rather than enlarging the milestone.
- **§6.3's fence forwarding requires changing the `SubAgentRequest` shape** in a way that reaches the durable payload. Report it; the payload shape is a compatibility surface.
- **The team-policy determination needs durable daemon state that does not exist yet.** Report the missing state.
- **Any fix would weaken an invariant in §4**, in particular invariant 3 or invariant 4.

## 15. Closure evidence required

1. A closure record at `plans/closure/agent-workstream-m005-writer-containment-corrective/009-status.md`, with the sixteen-section structure the closure template requires.
2. A requirement-to-evidence matrix mapping each of §13's six criteria to the test or command that demonstrates it.
3. The exact verification commands and their results, labelled honestly as local or hosted.
4. An invariant review confirming §4's eight invariants are intact.
5. A statement of which configurations can still reach the nested in-process dispatch after §6.3, and what now happens there. §3.3's exposure note must be discharged by evidence, not by assertion.
6. A registry update in the same commit as the closure.

## 16. Handoff notes

- The three defects were found by a 2026-10-23 review of `origin/main` at `f1b83a7a7`. All three are minors; the same review's agent-loop audit returned PASS on the six areas it covered.
- The tree at `f1b83a7a7` is **green**: `codegg --lib tool::writer_fence` (3), `codegg-core --test writer_permit` (21), `codegg --test shared_checkout_c006` (16), `codegg --test contained_worktree_c008` (5) and `codegg --test composed_two_agent_race_m011` (3) all pass. These defects are latent, not caught.
- §3.2 is the highest-value item: the `AuthorizationService` seam already exists in the permit authority, and `local_owner` is authorised for *any* capability on *any* project. This milestone does not wire that seam, but it must not leave the fallback in a state where wiring it would be unsafe.
- C007 and C008 of this line are the sibling corrective milestones for shared-checkout placement and the mediation authority. They are independent and must not be folded in.
- The shared checkout remains **opt-in** and **platform-gated**. Nothing here enables the WorkOrder `shared`/`serialized` mutation policy, which still fails closed.
