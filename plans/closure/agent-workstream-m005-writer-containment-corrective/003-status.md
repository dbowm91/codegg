# M005 Writer-Containment Corrective C003 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/agent-workstream-m005-writer-containment-corrective/003-native-file-and-git-writer-interposition.md`
Source subsystem roadmap: `plans/subsystems/agent-workstream-blackboard-m005-writer-containment-corrective-addendum.md` (C003)
Repository baseline reviewed: `6717d30b` (post-C002, post-M006/M007 closure)
Decision: `plans/adrs/ADR-0042-restricted-agent-shared-checkout-writer-contract.md` (**accepted 2026-10-19, option B**)
Predecessor closure: `plans/closure/agent-workstream-m005-writer-containment-corrective/002-status.md`

## 1. Executive finding

C003 put the writer fence in the writer path.

C002 shipped a permit that nothing consulted, and said so plainly. C003 is
the milestone that closes that gap: the fence is consulted at the **final
native effect** — the `fs::write`, the `fs::rename`, the Git index and ref
mutation — so the permit is taken *after* approval and *before* the first
effect, held across the write and any rollback, and released only with
evidence.

Two rules do the work, and they are different jobs:

- **Containment.** Every effect target must resolve inside the run's own
  checkout. The comparison is on canonical paths, so a symlink pointing out
  of the checkout buys nothing, and a *new* file cannot escape by naming a
  directory that does not exist. The roots the daemon granted alongside the
  checkout — the run's private scratch root and its shared scratch roots —
  are part of the scope, because a scratch write is a legitimate write that
  is not a checkout write and the model never names those roots.
- **Mutual exclusion.** A checkout shared by more than one run admits one
  writer at a time. The share is not asserted by a caller: it is derived from
  the durable group record. Two runs share a checkout when they are members
  of the same *live* group and the lease belongs to one of them. The group is
  the bounded reservation; the lease still fences the generation, and the
  permit still records the individual run — a group is not a bearer token.

**C003 is still not shared-write readiness, and it did not become one.** No
production path places two runs on one checkout: the work-order
`shared`/`serialized` placement is still refused at the coordinator, and the
scheduler still gives every delegated run its own isolated worktree. The
shared profile is a *capability* that C004, C005 and C006 complete and that
C006 must make reachable. The ten qualification tests construct the shared
case directly, which is the point: the fence is proved before any product
path is allowed to depend on it.

What C003 *does* change in production today is the containment rule. Before
C003, the `git` tool accepted a model-supplied `workdir` that was not
constrained to the checkout the run owns, so a model could point the tool at
any directory on the machine and stage, commit or reset a repository it did
not own. That hole is closed.

### What is live in production today, and what is not

This distinction is the one that matters for reading the rest of this record.

**Live: the containment rule.** Every agent loop — the daemon turn
(`src/agent/agent_loop_factory.rs`), the delegated-run worker
(`src/agent/worker.rs`), `codegg exec` (`src/exec.rs`) and the CLI
(`src/main.rs`) — now builds a `WorkspaceWriterFence` over the run's own
workspace and hands it to every tool execution context. A native writer whose
target resolves outside that scope is refused **before** the effect. That
holds today, in production, for `write`, `edit`, `replace`, `apply_patch`,
`lsp_preview_apply` and the typed `git` mutations.

**Not live: the permit-backed shared profile.** `DaemonWriterFence` — the
adapter that resolves the authoritative checkout scope and takes the durable
permit — currently has **no production caller**. It is exercised by the
qualification suite and nowhere else, because nothing in production places two
runs on one checkout.

That is the milestone's boundary, not an oversight. Wiring the permit-backed
profile into production is C006's job, because C006 is what creates the
shared placement; wiring it earlier would enable shared mutation before C004
and C005 have qualified the profile. The addendum is explicit: "C003–C006
must land and qualify before any shared mutation exists."

C002's own closure said the permit existed "without fencing anything". C003
is not a repeat of that: the fence *is* in the writer path and the
containment rule *does* refuse writes. What C003 does not yet do is make the
permit-backed profile reachable from a product path.

## 2. Requirement-to-evidence matrix

| Plan requirement | Where satisfied | Evidence |
|---|---|---|
| A — discover the authoritative checkout scope at tool execution, from daemon records, never from a model-provided path | `src/tool/writer_fence.rs` (`CheckoutWriterScope`); `src/core/writer_fence.rs` (`DaemonWriterFence::shared_group`, `worktree_id`); the scope is built from the run's `AgentRunRecord.worktree_id`, the `WorktreeService` lease, and the `AgentRunGroupService` record | `two_runs_on_one_checkout_never_overlap_their_writes`; `a_run_outside_the_group_may_not_write_the_shared_checkout` |
| A — a forged path, worktree or group cannot mint a permit | The scope is daemon-resolved; the permit's scope is resolved by `WorktreePermitAuthority::resolve_scope` from the worktree record and the run, and the group must be *live* and must actually contain the requesting run | `a_run_outside_the_group_may_not_write_the_shared_checkout` |
| A — a symlinked or linked checkout resolves to the canonical worktree id before the effect | `resolve_within_scope` canonicalizes both the target and the root, and resolves a not-yet-existing target through its nearest existing ancestor | `a_write_outside_the_run_checkout_is_refused_before_any_effect`; `a_granted_scratch_root_is_inside_the_scope` |
| A — backward compatibility for old isolated runs | A context with no fence takes the pre-C003 path unchanged: `acquire_writer_permit` returns `Ok(None)` and no effect is fenced | `an_isolated_checkout_takes_no_permit` |
| B — acquire the permit AFTER approval and BEFORE the first mutation; hold it through write / rename / rollback | `src/tool/write.rs`, `src/tool/edit.rs`, `src/tool/replace.rs`, `src/tool/apply_patch.rs`, `src/tool/lsp_preview_apply.rs`: the acquire sits between the last validation and the first `fs::write` / `fs::rename` / `remove_file`, and the settle sits after the effect | `the_write_tool_refuses_an_out_of_scope_target`; `two_runs_on_one_checkout_never_overlap_their_writes` |
| B — permit granularity for a batch; no double acquisition in nested dispatch | One permit per tool call, covering every file that call touches (`apply_patch` move names both ends). The broker *carries* the fence but never acquires from it, so a tool that never reaches an effect holds nothing | `two_runs_on_one_checkout_never_overlap_their_writes` |
| B — no permit held during approval, inference or child scheduling | The fence is consulted only at the effect; the acquire is the last thing before the mutation and the settle the first thing after it | `an_out_of_scope_write_is_refused_before_the_permit_is_taken` |
| C — instrument the typed Git index / worktree / ref mutation, stage / commit, integration and recovery | `src/tool/git.rs`: `dispatch_mutation` classifies the action, takes the permit immediately before the typed executor, and settles after | `a_write_outside_the_run_checkout_is_refused_before_any_effect` (containment is profile-independent) |
| C — no automatic merge; preserve typed Git authorization | The typed executor and its authorization are unchanged; the fence adds a refusal in front of it and never widens it | `architecture/writer_permit.md` ("The checkout writer fence") |
| C — do not hold the checkout permit while awaiting a child job or external approval | The permit is scoped to one effect and settled immediately after it; no path holds it across a submission | `architecture/writer_permit.md` ("The checkout writer fence") |
| D — disabled / unknown native effect classes fail closed for any requested shared profile | `DaemonWriterFence::begin`: `NativeEffectClass::Unknown` has no permit operation and `ProcessSpawn` is refused on a shared profile. The `git` raw argv fallback is routed through the fence as `Unknown` so it cannot slip past the typed path | `an_unclassified_effect_is_refused_on_a_shared_checkout` |
| D — unsupported network / ref mutators never fall back to raw shell or a compatibility Git path | The raw fallback is classified `Unknown` and routed through the fence; there is no untyped write path left in the `git` tool | `an_unclassified_effect_is_refused_on_a_shared_checkout` |
| D — maintain the previous isolated-worktree behaviour without new expensive acquisition by default | An isolated checkout takes **no** permit: no durable write, no wait, no lock | `an_isolated_checkout_takes_no_permit` |
| E — typed audit / journal entries and bounded status for acquire / effect / release / deny, with exact run / attempt attribution and redaction | Acquire and release are journalled durably by C002's `writer_permit` store, which carries the owner token, revision, lease generation, operation, run id and attempt id, and projects a redacted operator view. The fence's decisions additionally emit bounded `tracing` events carrying the worktree id, run id, effect class and reason — never a path, a file body or a model string | §10 finding 1 (the deny side has no durable *event* row) |
| E — daemon restart reconciles uncertain in-flight effects to quarantine rather than replay | C002's `WriterPermitStore::reconcile_after_restart`, run by the daemon at startup; an unsettled permit is parked, never replayed | `tests/writer_permit_service.rs::startup_reconciliation_parks_a_permit_left_live` |
| E — `codegg-core` holds the typed IDs, store and state machine; the root daemon adapter supplies authorization | `crates/codegg-core/src/writer_permit.rs`; `src/core/writer_permit.rs`; the new `src/core/writer_fence.rs` sits on the daemon side and `src/tool/writer_fence.rs` never imports `crate::core` | `scripts/check-core-boundary.sh`; `grep -rn "crate::core" src/tool/` (no hits) |
| Stop condition — no effect can occur before the permit or without group scope | The acquire precedes the first mutation in every fenced tool; an out-of-scope target is refused *before* the permit is taken, so a refused write never holds the checkout | `an_out_of_scope_write_is_refused_before_the_permit_is_taken` |
| Stop condition — no broad opt-in enablement | Nothing in production places two runs on one checkout; the work-order `shared`/`serialized` policy is still refused at the coordinator and the scheduler still isolates every delegated run | §3.3 |

### Required qualification — what was run and what was not

| Required | Run | Result |
|---|---|---|
| Two real AgentRun contexts on one checkout; barrier overlap; one active writer, second waits / typed-denied, readers proceed | `tests/writer_fence_c003.rs` | **Partially.** The two-run case is proved: the second writer is refused with a typed `Held` while the first holds the checkout, and takes it only after the first settles. The *readers proceed* half is not proved against a shared checkout: the shared profile is a capability with no production path, and the qualification constructs it directly rather than through a product route. |
| Native file vs native file; native file vs Git stage / commit; Git vs Git recovery | — | **Not run.** The qualification drives the fence directly, not the tools end-to-end against a shared checkout. This is a real gap; see §10 finding 2. |
| Cancellation before the first effect; during rename / rollback; after commit before release; stale lease / restart; invalid group membership | — | **Partially.** The fence's own cancellation and invalid-membership behaviour is typed and refused (`Cancelled`, `GroupReservationInactive`), but no test injects cancellation into a held permit. See §10 finding 3. |
| Distinct worktrees in parallel | `tests/writer_permit_service.rs::distinct_worktrees_acquire_in_parallel` | Passed (C002). |
| A production-shaped temporary Git repository, not mocks alone | `tests/writer_fence_c003.rs` | **Partially.** The checkout is a real directory and the worktree row is a real leased row in a real SQLite database, but the qualification does not drive a real `git` command. See §10 finding 2. |

## 3. Production implementation evidence

### 3.1 The tool-facing contract — `src/tool/writer_fence.rs` (new)

The contract the tool layer codes against. It holds no permit state, resolves
no identity and reads no store; the daemon adapter owns all of that. It
depends on `codegg_core` types only and **never imports `crate::core`**, so
the tool layer stays independent of the daemon.

- `CheckoutWriterScope` — the daemon-resolved scope: the managed checkout's
  id, its canonical root, the owning run, and the sharing group.
- `NativeEffectClass` — `FileWrite`, `GitMutation`, `ProcessSpawn`, `Unknown`.
  `Unknown` is not "probably fine"; it is refused under a shared profile.
- `WriterFenceError` — seven typed refusals: `ScopeMissing`,
  `OutsideScope { path, root }`, `EffectClassDenied { class, reason }`,
  `Held`, `Cancelled`, `Quarantined`, `Unavailable`.
- `WriterFenceOutcome` — `Settled`, `NoEffect`, `Unproven(String)`. `Unproven`
  does not free the checkout; it parks it.
- `CheckoutWriterFence` — `scope()` and `begin(request)`. `begin` returns
  `Ok(None)` when the effect is in scope and the checkout is not shared, so
  the caller performs the effect exactly as it did before.
- `WorkspaceWriterFence` — the containment-only implementation used by the
  daemon turn, the delegated-run worker, `codegg exec` and the CLI: it holds
  no managed checkout, so it can contain but can never mint a permit.
- `acquire_writer_permit` / `settle_writer_fence` / `writer_fence_key` — the
  helpers the tools use, so no tool re-implements the ordering rule.
- `granted_roots` — the daemon-granted roots (private scratch root and shared
  scratch roots), read from the execution's authorization context. The model
  never names them.

### 3.2 The daemon adapter — `src/core/writer_fence.rs` (new)

`DaemonWriterFence` implements the contract against the daemon's own stores.

1. **Containment first.** The targets are resolved against the scope's
   canonical root and the granted roots before anything else. An
   out-of-scope target is refused here, so no permit is taken and no effect is
   attempted.
2. **No managed checkout, no permit.** A run with no `worktree_id` has no
   lease, so there is nothing to serialize. Containment is the whole fence.
3. **The share is derived, not asserted.** `shared_group` reads the worktree
   record's owner run and asks the group service for the groups each run
   belongs to; a common *live* group is what makes the checkout shared.
4. **Fail closed.** `Unknown` has no permit operation; `ProcessSpawn` is
   refused on a shared profile. Neither falls back to a raw path.
5. **No anonymous writer.** A shared write with no bound principal — read
   from the trusted execution-audit context, never from model text — is
   refused.
6. **Take the permit.** The request carries the worktree id, the principal,
   the run, the group, the attempt, the operation class, the idempotency key
   and a bounded wait with the run's cancellation token.

`PermitSettler` releases with the evidence the outcome implies: `Settled` and
`NoEffect` free the checkout, `Unproven` parks it. The daemon's log records
the fence's decisions with the worktree id, run id, effect class and reason —
never a path, a file body or a model string.

### 3.3 The scope of the shared profile, precisely

`WorktreePermitAuthority::resolve_scope` previously refused any run that did
not hold the checkout's lease. It now also admits a run that **shares a live
group with the lease owner**. That is the whole of the widening: it widens
*who* may write a shared checkout, never *what* a writer may do. The group
is the bounded reservation; the lease still fences the generation; the
permit still records the individual run.

Nothing in production exercises that path yet. The work-order coordinator
still refuses `Shared`/`Serialized` placement with a mutation at the
coordinator (`shared_mutation_unavailable`), and `SubagentJobExecutor` still
creates a fresh managed worktree for every delegated mutating run. C003
therefore **enables no shared mutation**, exactly as the addendum requires
("C003–C006 must land and qualify before any shared mutation exists").

### 3.4 The fenced surfaces

| Surface | Where the fence sits | Notes |
|---|---|---|
| `write` | `src/tool/write.rs`, before the first `fs::write` | The scratch branch re-roots the tool and the fence follows the root the write actually uses. |
| `edit` | `src/tool/edit.rs`, before the first `fs::write` | The write step keeps its second path resolution; the fence names the canonical path. |
| `replace` | `src/tool/replace.rs`, before the first `fs::write` | The check-then-write window is unchanged: the re-read and the write stay inside one blocking closure. |
| `apply_patch` | `src/tool/apply_patch.rs`, in all four modes | `move` names both ends of the rename. |
| `lsp_preview_apply` | `src/tool/lsp_preview_apply.rs`, before `apply_preview` | The targets are the files the host-owned candidate touches; the model supplies only a preview id. |
| `git` typed mutations | `src/tool/git.rs`, before the typed executor | Read-only actions take no permit and are never refused. |
| `git` raw argv fallback | `src/tool/git.rs`, before the command | Classified `Unknown` so it fails closed on a shared checkout. |

The broker carries the fence and never acquires from it. A read, a denied
call, or a job submission holds no permit and writes nothing — which is also
what keeps a nested dispatch from acquiring the same checkout twice.

## 4. Verification executed

See §4.4 for the environment note that applies to the workspace-wide sweep.

### 4.1 The C003 qualification suite

`tests/writer_fence_c003.rs` (10 tests):

- `a_write_outside_the_run_checkout_is_refused_before_any_effect`
- `a_write_inside_the_run_checkout_is_allowed`
- `a_granted_scratch_root_is_inside_the_scope`
- `the_write_tool_refuses_an_out_of_scope_target`
- `two_runs_on_one_checkout_never_overlap_their_writes`
- `a_run_outside_the_group_may_not_write_the_shared_checkout`
- `an_unclassified_effect_is_refused_on_a_shared_checkout`
- `a_shared_write_without_a_bound_actor_is_refused`
- `an_out_of_scope_write_is_refused_before_the_permit_is_taken`
- `an_isolated_checkout_takes_no_permit`

### 4.2 The workspace sweep

`cargo fmt --all -- --check` — **pass**.

`scripts/verify.sh quick` — **pass**. Every guard in the quick sweep passed
and the sweep's `cargo check --workspace --all-targets --locked` finished
clean.

`cargo clippy --workspace --all-targets -- -D warnings` — see §4.4.

The scoped suites that cover the changed surfaces:

| Suite | Result |
|---|---|
| `tests/writer_fence_c003.rs` (new) | 10 passed, 0 failed |
| `tests/writer_permit_service.rs` (C002, unchanged in behaviour) | 12 passed, 0 failed — including `a_run_without_the_lease_is_refused`, which is the cross-tenant denial and would have broken if the group-sharing rule had widened it |
| `tests/tool_broker_integration.rs` | 25 passed, 0 failed |
| `cargo test -p codegg --lib -- tool::write tool::edit tool::replace tool::apply_patch tool::git tool::lsp_preview_apply` | 36 passed, 0 failed |

### 4.3 Pre-existing defects touched in passing

None. C003 changed no pre-existing behaviour that a previous milestone
depended on, and no pre-existing lint had to be fixed in passing. The one
compilation failure the first `verify.sh quick` run found — `tests/causal_active_m005.rs`
and 13 other test call sites of `AgentLoop::new` missing the new fence
argument — is a consequence of C003's own signature change, not a pre-existing
defect; it was repaired in the same change.

### 4.4 Environment note

The host is shared with other sessions' `cargo` builds, and the shared build
directory's lock stalls a workspace-wide sweep. The C003 work was therefore
verified in a dedicated target directory (`CARGO_TARGET_DIR=/tmp/codegg-c003-target`)
for the test and clippy runs, and in the shared directory for
`scripts/verify.sh quick`.

## 5. Invariant review

- **The fence never widens.** Every refusal is *additional* to the
  permission, sandbox and Git-authorization layers. The fence cannot enable a
  tool, a path or an actor that another layer refused.
- **The permit is taken after approval and before the first effect, and
  nowhere else.** No permit is held during approval, provider inference, or
  child scheduling, because the acquire is the last statement before the
  mutation.
- **The permit is held through the effect.** The settle is after the write,
  the rename and any rollback, so a second writer cannot enter the checkout
  while the first writer's effects are still landing.
- **Absence of evidence is not evidence.** `Unproven` parks the checkout.
  A failure that cannot prove its effects settled never frees the checkout
  for the next writer.
- **The group is not a bearer token.** The permit's scope records the
  individual run, the attempt and the lease generation. A group authorizes
  *who* may write a shared checkout; it never becomes the writer's identity.
- **No new durable cost by default.** An isolated checkout takes no permit:
  no CAS, no wait, no row.
- **The shared profile fails closed.** An unclassified effect, a process
  effect and an unattributed writer are all refused before any effect.

## 6. Failure and recovery review

| Failure | Behaviour |
|---|---|
| The target resolves outside the checkout | Refused before the permit is taken; the checkout is never held; nothing lands on disk. |
| The checkout is held by another writer | The second writer is refused with the typed `Held`; it waits for a bounded window and is refused on timeout. |
| The permit is quarantined | The writer is refused with the typed `Quarantined`; the checkout stays parked until an operator releases it. |
| The writer's effects cannot be proven to have settled | The permit is quarantined, not released. The checkout needs operator recovery. |
| The daemon restarts with a live permit | C002's restart reconciliation parks the permit. It is never replayed: repairing metadata by re-running a mutation would be a second effect, not a repair. |
| The lease moved under the writer | The permit's lease generation no longer matches; the store refuses the release and the permit stays with its holder. |
| The group ends while a writer holds the checkout | The group is no longer live; a new writer is refused. The holder's own release still resolves against the permit's scope. |
| The execution is cancelled while waiting for the permit | The wait is cancelled with the run's cancellation token; the waiter holds nothing. |

## 7. Migration and compatibility review

- **No schema change.** C003 adds no table, no column and no migration. The
  group-sharing rule is derived from the existing `agent_run_group` record and
  the existing `managed_worktree` lease.
- **No protocol change.** The fence is internal to the tool and daemon
  layers; no wire type, no event variant and no CLI surface changes.
- **Backward compatible.** A `ToolExecutionContext` without a fence takes the
  pre-C003 path. Every existing tool test that calls `execute` directly
  without a context is unaffected.
- **The `git` tool's behaviour narrows in one specific way**: a *mutating*
  `git` action whose resolved repository root is outside the run's checkout
  is now refused. That is the intended closing of the model-supplied
  `workdir` hole, and it is a narrowing, not a widening.

## 8. Security review

- **The model cannot mint a permit.** The scope is resolved by the daemon
  from the run record, the worktree lease and the group record. The permit's
  own authority re-resolves the scope from the same stores and refuses a
  request whose group does not actually contain the requesting run.
- **The model cannot escape the checkout.** Containment compares canonical
  paths, and a not-yet-existing target is resolved through its nearest
  existing ancestor, so neither a symlink nor a missing directory buys an
  escape. The granted scratch roots are daemon-owned; the model asks to write
  under a root, it does not name one.
- **No anonymous writer.** A shared write with no bound principal is
  refused. The principal comes from the trusted execution-audit context and
  is never synthesized from a model string.
- **Fail closed on the unclassified.** The `git` raw argv fallback is
  classified `Unknown` and refused on a shared checkout, so there is no
  untyped write path left in the `git` tool.
- **Redaction.** The fence's log events carry the worktree id, run id, effect
  class and reason. They carry no path, no file body, no model text and no
  possession token; the permit's operator view is already redacted.

## 9. Documentation and operations

- `architecture/writer_permit.md` — a new "The checkout writer fence (C003)"
  section: the two rules, the fenced surfaces, the two profiles, and what is
  not yet true. The "What This Does Not Claim" section was corrected; it used
  to say no writer consults the permit, which C003 made false.
- `architecture/overview.md` — the Checkout Writer Permit row was corrected
  for the same reason.
- `plans/implementation/agent-workstream-m005-writer-containment-corrective/003-native-file-and-git-writer-interposition.md` — the status line was advanced from "ready … not started" to closed, with this record as its closure.

## 10. Unresolved findings

| Severity | Finding | Disposition |
|---|---|---|
| medium | The **deny** side of the fence has no durable *event* row. Acquire and release are journalled by C002's `writer_permit` store, and the fence's decisions emit bounded `tracing` events, but a refusal that never reached the store leaves no durable audit row. A durable deny event needs a new `AuditAction` in `codegg-core`'s event-coverage matrix, which is a contract change with its own coverage rules. | Accepted for C003. The refusal is typed, returned to the caller as a non-retryable `ToolError::Permission`, and logged with the exact scope. The durable audit-event surface for the whole restricted writer profile is assigned to **C004**, which owns the non-broker sinks and therefore the audit surface for them. |
| medium | The qualification drives the **fence**, not the tools end-to-end against a shared checkout. The "native file vs native file" and "native file vs Git stage/commit" barrier cases, and a real `git` command against the shared checkout, are not exercised. | Accepted for C003 with a scope note. C003's contract is that the fence is consulted at the effect and that the permit serializes the checkout; both are proved. The end-to-end shared-checkout trajectory needs a production path to exist first, which is **C006**'s job. C006 must run the full barrier matrix. |
| low | No test injects cancellation into a **held** permit (cancelled before the first effect, during a rename or rollback, after a commit but before the release). The fence's wait is bounded and cancellation-aware, and the permit's own cancellation behaviour is covered by C002, but the combination is not. | Accepted. Assigned to **C006** together with the barrier matrix, where the shared checkout is real. |
| low | The `write` tool's scratch branch re-roots the tool, and the fence follows the root the write actually uses. The daemon's scope resolution therefore has to include the granted scratch roots for the containment to be meaningful, and it does — but the granted roots are read from the *execution's* authorization context rather than from the fence's own scope. A future caller that builds a fence without an authorization context would contain a scratch write. | Accepted. The helper derives the granted roots from the context it is handed, so the two cannot drift. Documented in `architecture/writer_permit.md`. |
| low | The `git` tool's read-only actions are deliberately not fenced. A read is not a write, and the plan says readers overlap freely — but a read can still observe another writer's half-applied state. | Accepted. It is the plan's explicit intent. A reader that needs a consistent snapshot is a read-transaction concern, not a writer concern. |

## 11. Roadmap disposition

- **C003: closed.** The fence contract, the daemon adapter, the authoritative
  scope resolution, the containment rule, the fail-closed effect classes, the
  group-sharing rule in the permit's authority, the routing of every native
  file writer and the typed Git mutations, and the qualification evidence all
  exist.
- **C004 (non-broker writer sink mediation): unblocked** by this closure,
  subject to its own predecessor check. C004 owns the non-broker sinks, the
  durable audit-event surface for them, and the remaining `fenced-native`
  manifest roots that are not native tool routes (`src/document_service.rs`,
  `src/core/daemon_project_init.rs`, `crates/codegg-core/src/snapshot/checked_restore.rs`,
  `crates/codegg-core/src/worktree_service.rs`,
  `crates/codegg-core/src/goal/checkpoint.rs`).
- **C005 (process and persistent-service deny contract): still blocked** on
  its own ordered predecessor. C003's fence refuses `ProcessSpawn` on a shared
  profile, which is the fail-closed direction; C005 owns the contract itself.
- **C006 (restricted shared checkout qualification): still blocked** on its
  own ordered predecessors, and it now carries the barrier matrix that C003's
  qualification deferred.
- **Parent M005: still not closed.** C001–C006 and the Q1–Q5 evidence are all
  still required, and C006 must produce a real production path.
- **Independent M006/M007: unaffected.** C003 changed no isolated-worktree
  placement behaviour and no build-admission behaviour.

## 12. Registry updates

- `plans/registry.md` — the M005 writer-containment corrective row advances
  to "C001–C003 closed", names the fence and the containment rule as C003's
  delivery, and states plainly that the shared profile is a capability with no
  production path.
- `plans/subsystems/agent-workstream-blackboard-m005-writer-containment-corrective-addendum.md`
  — the C003 row advances to closed, with this record as its closure.
- No downstream milestone became newly eligible other than C004, which was
  already dependency-ordered behind C003.
