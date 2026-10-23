# Agent Workstream M005 Writer-Containment Corrective C009 — Agent-Loop Writer Fence Completeness: Status

Status: closed

Milestone: `plans/implementation/agent-workstream-m005-writer-containment-corrective/009-agent-loop-writer-fence-completeness.md`
Review baseline: `f1b83a7a7` (`origin/main`, 2026-10-23) — the tree §3's evidence was gathered against
Implementation baseline: `f2639b5d2` (`origin/main`, 2026-10-23) — the tree this work was applied to

## 1. Executive finding

Three ways a write could reach a shared checkout without the authority the
fence exists to enforce. Each was a small hole; together they meant the fence
was not the last line it claimed to be.

**The `commit` tool was the one mutating tool registered without the
fail-closed wrapper.** `ToolRegistry::with_options` documents that the `None`
arms **refuse** rather than defaulting, and fourteen workspace tools honour it.
The commit tool did not: with `workspace_root: None` it stayed in the registry
with the sentinel workdir `\0codegg:no-workspace-root`, which
`canonicalize_existing_prefix` resolves to `<checkout_root>/\0codegg:no-workspace-root`
— lexically **under** the root, so the fence's containment check **passed**. The
tool then took the checkout's writer permit, failed with a raw Git error, and
reported `Unproven` — which **parks the checkout** rather than releasing it. The
commit tool now takes the same `None => unrooted(...)` arm as its siblings, so
a missing root is a typed refusal and no permit is taken.

**The daemon's team-policy refusal was unreachable.** The
`local_principal_fallback` flag's documented contract is that a daemon with a
team policy leaves it **off**, so a write arriving without a bound actor is
refused rather than written anonymously. The only production construction site
passed it unconditionally as `true`. The daemon **has** a durable team model
(`crates/codegg-core/src/team`), but the flag never consulted it — so every
shared-checkout write was attributed to the synthetic
`AuthenticatedPrincipal::local_owner("codegg-writer-fence")`, and the permit
row could not distinguish two writers. The "a shared checkout has no bound
actor" refusal was **dead code**.

The flag is now **derived from the daemon's durable team state**, read at fence
time so it survives a restart and follows a membership change without one. An
unreadable team state is read as "has a team policy" — the refusing direction,
because reading it as "single-user" would restore exactly the synthetic
attribution this milestone removes.

**The nested-subagent dispatch dropped the fence.** The `task` tool's
in-process spawner built its request with `writer_fence: None`, and the worker
substituted a containment-only fence. `WorkspaceWriterFence` is
**containment-only**: its `begin` returns `Ok(None)` and can **never** take a
permit. The nested child inherits the parent's workspace root — which, for a
delegated run, **is** the managed checkout — so a grandchild that wrote wrote
into the shared checkout **without taking the writer permit**, overlapping the
group sibling that did hold it. The dispatch now forwards the parent's fence,
and the worker's fallback is guarded by a managed-checkout probe: a run on a
managed checkout with no fence is **refused**, never silently downgraded.

## 2. Requirement-to-evidence matrix

| §13 criterion | Evidence | Result |
|---|---|---|
| 1. A registry built with `workspace_root: None` refuses **every** mutating tool, including `commit` | §4.1 | **met** |
| 2. A daemon with a team policy installs no local-principal fallback, refuses an unattributed shared-checkout write, and binds the transport principal so two writers on one shared checkout are distinguishable | §3.1, §3.2 and §4.2 | **met, with one named limit — §10** |
| 3. A single-user daemon still attributes its own writes and is not refused by its own fence | §4.3 | **met** — this is the non-regression the milestone had to protect |
| 4. A nested child dispatched from a run on a managed checkout takes the checkout's writer permit or is refused with a typed error — never silently downgraded to containment-only | §3.3 and §4.4 | **met** |
| 5. Every defect above has a regression test, and every new guard is wired in with a `--self-test` case that is rejected when the defect is reintroduced | §4 and §5 | **met** |
| 6. `verify.sh quick`, workspace clippy and the named test targets are green | §4.5 and §4.6 | **met** — `verify.sh quick` is green across its 86 steps, the workspace clippy with `-D warnings` is green, and the three named test targets are green |

## 3. Production implementation evidence

### 3.1 The commit tool takes the fail-closed arm

`src/tool/mod.rs` — the commit tool now registers through the same wrapper as
the other fourteen workspace tools:

```rust
// M005 corrective C009: `commit` is a mutating workspace tool, so it
// takes the same fail-closed `None` arm as the fourteen workspace tools
// above it. …
let commit_tool: Box<dyn Tool> = match workspace_root.as_ref() {
    Some(_) => Box::new(commit_tool),
    None => unrooted(Box::new(commit_tool)),
};
registry.register_boxed(commit_tool);
```

The root-present arm is unchanged: `CommitTool::new().with_workdir(root.clone())`.

### 3.2 The principal fallback is derived, and the transport principal is bound

**Derivation.** `src/core/writer_fence.rs` gained
`DaemonWriterFenceFactory::with_team_state(..)` and
`effective_local_principal_fallback()`:

```rust
async fn effective_local_principal_fallback(&self) -> bool {
    let Some(store) = self.team_store.as_ref() else {
        return self.local_principal_fallback;
    };
    match store.list_principals().await {
        Ok(principals) => principals
            .iter()
            .all(|principal| principal.kind == codegg_core::team::PrincipalKind::LocalOwner),
        Err(error) => {
            tracing::warn!(
                error = %error,
                "team policy probe failed; unattributed shared-checkout writes are refused"
            );
            false
        }
    }
}
```

`src/core/daemon_construct.rs` — the production site hands over the daemon's
**durable team store** instead of asserting the flag:

```rust
crate::core::writer_fence::DaemonWriterFenceFactory::with_team_state(
    permit.clone(),
    worktree_service.clone(),
    deps.agent_run_store.clone(),
    Some(deps.run_group_service.clone()),
    deps.pool
        .clone()
        .map(|pool| Arc::new(codegg_core::team::TeamStore::new(pool))),
)
```

The fallback stands only while every principal the team store names is the
local owner — the principal a single-user daemon bootstraps for itself. A
daemon that gains a member stops attributing shared-checkout writes to a
synthetic owner **without needing a restart**.

`DaemonWriterFenceFactory::new(.., bool)` is **kept unchanged** for the
harness call sites; only the production site moved.

**Binding.** `src/agent/tool_batch.rs` — the loop's execution context carries
a real principal instead of `execution_audit: None`. The daemon already
records the request's transport-bound principal as a durable **turn-scope
origin attribution** at the admission boundary (`src/core/daemon_turns.rs`),
and the loop restores that row:

```rust
async fn resolve_transport_execution_audit(
    &self,
) -> Option<codegg_core::audit_instrumentation::TrustedExecutionAuditContext> {
    …
    let principal = crate::tool::factory::principal_from_origin_attribution(&attribution);
    …
}
```

`src/tool/factory.rs` gained `principal_from_origin_attribution`, which
restores the principal from the durable row:

```rust
pub fn principal_from_origin_attribution(
    attribution: &codegg_core::authorization::OriginAttribution,
) -> codegg_core::transport_auth::AuthenticatedPrincipal {
    codegg_core::transport_auth::AuthenticatedPrincipal::reconstructed(
        attribution.origin_principal.clone(),
        attribution.origin_kind,
        attribution.auth_method,
        attribution.transport_class,
        RESTORED_ORIGIN_CLIENT_ID,
    )
}
```

The principal comes from the daemon's **trusted transport** — never from the
model, and never from anything the model can influence. The legacy marker
records work that predates attribution and names no team identity, so it is
**not** promoted into one.

### 3.3 The nested dispatch forwards the fence, and the fallback is guarded

**Forwarding** (`src/tool/task.rs`) — `TaskTool` gained a `writer_fence`
field and `with_writer_fence(..)`, and the in-process spawner now carries the
parent's fence into the `SubAgentRequest`:

```rust
// M005 corrective C009: forward the parent's checkout
// writer fence. The nested child inherits the parent's
// workspace root, which for a delegated run IS the
// managed checkout; dropping the fence here left the
// child writing into the shared checkout without
// taking the writer permit, overlapping the group
// sibling that holds it.
writer_fence: self.inherited_writer_fence(ctx),
```

`SubAgentRequest` already carried the `writer_fence` field, so the **durable
payload shape did not change** — the plan's §14 stop condition was not
triggered.

**The fallback** (`src/agent/worker.rs`) — the unconditional containment-only
substitution is gone:

```rust
let writer_fence: Option<crate::tool::writer_fence::SharedWriterFence> =
    match request.writer_fence.clone() {
        Some(fence) => Some(fence),
        None => {
            let durable_runs = subagent_pool.durable_agent_runs.lock().await.clone();
            if request_holds_managed_checkout(durable_runs, request).await {
                return Err(
                    "delegated run holds a managed checkout but no writer fence was resolved; \
                     refusing to downgrade to a containment-only fence"
                        .into(),
                );
            }
            Some(std::sync::Arc::new(
                crate::tool::writer_fence::WorkspaceWriterFence::new(workspace_root.clone()),
            ))
        }
    };
```

The probe's authority is the **durable run record** (`record.worktree_id`). A
run with no worktree holds no managed checkout and keeps the containment-only
fallback, so the single-user path does not regress into refusing itself.

## 4. Verification executed

All commands were run **locally**. No hosted lane was consulted, and none is claimed.

### 4.1 The registry refuses every mutating tool

`tests/writer_fence_c009.rs` gained
`a_registry_without_a_workspace_root_refuses_every_mutating_tool`: a registry
built with `workspace_root: None` refuses **every** mutating tool, `commit`
included, with the typed missing-root error.

Against the baseline, the commit tool was registered directly, kept the
sentinel workdir, passed the fence's containment check, and took the permit.

### 4.2 The team policy refuses, and two writers are distinguishable

`a_team_daemon_refuses_an_unattributed_shared_checkout_write` — a daemon whose
team store names a principal other than the local owner refuses an
unattributed shared-checkout write.

`two_writers_on_one_shared_checkout_have_distinguishable_principals` — two
writers on one shared checkout produce **distinguishable** permit rows.

### 4.3 The single-user path does not regress

`a_single_user_daemon_still_attributes_its_own_writes` — a daemon with no
team policy still attributes its own writes and is **not** refused by its own
fence. This is the non-regression the milestone had to protect: removing the
fallback outright would have made a single-user daemon refuse itself, which
would be a regression rather than a fix.

### 4.4 The nested dispatch

`src/tool/task.rs` gained `the_nested_dispatch_inherits_the_parents_writer_fence`.

`src/agent/worker.rs` gained two unit tests:

- `a_run_on_a_managed_checkout_is_never_downgraded_to_containment_only`;
- `a_run_with_no_managed_checkout_keeps_the_containment_only_fallback`.

### 4.5 The named test targets

| Target | Result |
|---|---|
| `cargo test -p codegg --test writer_fence_c009` | **4 passed, 0 failed** |
| `cargo test -p codegg --lib tool::writer_fence` | green |
| `cargo test -p codegg --lib agent::worker::writer_fence_tests` | green |
| `cargo test -p codegg --test core_daemon` | **507 passed, 0 failed** |
| `cargo test -p codegg --test shared_checkout_c006` | **19 passed, 0 failed** |

The same scoped run also executed the `server_feature` target, which reports
**100 passed, 1 failed**. That failure is
`projection_transport_real::real_tui_pending_snapshot_disconnect_convergence_50_cycles`,
a **pre-existing** flake in a file this milestone does not touch; it was
measured on the untouched baseline and its analysis is in
`plans/closure/agent-workstream-m005-writer-containment-corrective/008-status.md`
§10 (U1).

### 4.6 The whole-tree gates

`scripts/verify.sh quick` is **green** on the implementation tree — 86 steps,
including `cargo fmt --check --all`, every guard with its `--self-test`, and
`cargo check --workspace --all-targets --locked`. `cargo clippy --workspace
--all-targets --locked -- -D warnings` is **green** as well, after four lints
the new code introduced were fixed.

## 5. The guard

`scripts/check_writer_fence_completeness.py` is **new**, and asserts four
things, each with a matching injection in its `--self-test`:

| Rule | Asserts | Injection |
|---|---|---|
| `registry-fail-closed` | every mutating workspace tool registered in `ToolRegistry::with_options` is constructed behind the `unrooted(...)` wrapper | a mutating tool registered without the wrapper is caught |
| `local-principal-fallback` | the production site does not assert the flag as a literal `true` and builds the factory from the daemon's durable team state | a literal `true` at the production site is caught; a site that asserts the flag but never consults the team state is caught |
| `worker-fence-fallback` | the containment-only fence is never substituted without the managed-checkout probe | the unconditional substitution is caught |
| `nested-dispatch-fence` | the `SubAgentRequest` literal never carries `writer_fence: None` | a nested dispatch that drops the parent's fence is caught |

```
$ python3 scripts/check_writer_fence_completeness.py
writer-fence completeness check passed

$ python3 scripts/check_writer_fence_completeness.py --self-test
  ok   a mutating tool registered without the unrooted wrapper is caught
  ok   the same mutating tool behind the unrooted wrapper is not a violation
  ok   a read-only workspace tool registered without the wrapper is not a violation
  ok   the inline match form of a mutating registration is accepted
  ok   a literal true local-principal fallback at the production site is caught
  ok   a production site that derives the fallback from the team state is accepted
  ok   a production site that asserts the fallback but never consults the team state is caught
  ok   the unconditional containment-only substitution is caught
  ok   a containment-only fallback guarded by the managed-checkout probe is accepted
  ok   a nested dispatch that drops the parent's fence is caught
  ok   a nested dispatch that forwards the parent's fence is accepted

self-test passed: 11 case(s).
```

**Non-vacuity against the baseline.** The guard was run against the
pre-fix sources: it reports **6 findings** on the baseline tree — 1 ×
`registry-fail-closed` (the commit tool), 2 × `local-principal-fallback`,
2 × `worker-fence-fallback`, 1 × `nested-dispatch-fence` — and **0** on the
fixed tree.

The guard runs in `scripts/verify.sh quick` and in CI, with its `--self-test`.

## 6. Invariant review

§4's invariants, reviewed against the implementation tree:

| # | Invariant | Status |
|---|---|---|
| Every mutating tool refuses when the workspace root is absent | **restored** — the commit tool takes the fail-closed arm |
| The single-user daemon's own writes stay attributable | **intact** — the fallback stands while the team store names only the local owner, and is asserted by a test |
| An unreadable team state refuses rather than attributes anonymously | **intact** — the probe's error arm returns `false` |
| The fence's containment check is not weakened | **intact** — no containment arm was changed; the sentinel workdir is now unreachable rather than accepted |
| The nested dispatch's fallback is not a silent downgrade | **restored** — the fallback is guarded by a durable managed-checkout probe |
| The durable payload shape is unchanged | **intact** — `SubAgentRequest` already carried `writer_fence` |
| The `AuthorizationService` seam is not wired | **intact** — it was explicitly out of scope; the `Capability::FileModify` check is still skipped, and the team-policy refusal carries the milestone instead |

## 7. Failure, cancellation, restart and contention semantics

**A missing workspace root refuses before any permit is taken.** The commit
tool's typed refusal happens in the wrapper, so the checkout is never parked
in quarantine for a root it never had.

**A team daemon refuses an unattributed shared-checkout write.** The refusal
is now **reachable**: the team store's membership is consulted at fence time.

**A membership change takes effect without a restart.** The derivation reads
the durable team store on each fence call, not once at construction.

**A restart preserves the derivation.** The team state is durable, so a
restarted daemon derives the same answer.

**An unreadable team state refuses.** The failing direction is the refusing
one.

**A delegated run on a managed checkout with no fence is refused.** The
refusal is typed and happens before the run executes.

## 8. Compatibility and migration review

No migration. No schema change. No durable payload shape change.

Two observable behaviour changes, both intended:

- the commit tool with no workspace root now **refuses** with a typed error
  where it previously took a permit and parked the checkout in quarantine;
- a team daemon's unattributed shared-checkout write is now **refused** where
  it was previously written under the synthetic local owner.

`DaemonWriterFenceFactory::new(.., bool)` is kept, so the harness call sites
and any external constructor are unaffected.

## 9. Security review

The direction of every change is "refuse rather than attribute anonymously".
Concretely:

- the commit tool can no longer acquire a permit under a sentinel workdir
  that only *looks* contained;
- a daemon with a team policy can no longer write a shared checkout without a
  bound actor, and two writers on one checkout now produce distinguishable
  permit rows;
- a grandchild dispatched from a run on a managed checkout can no longer write
  into that checkout without taking the permit the group sibling holds.

The one place the milestone deliberately does **not** widen authority is the
single-user daemon: its fallback is derived, not removed, so a single-user
daemon is not refused by its own fence.

## 10. Unresolved findings

**U1 — a delegated child's own principal is still unbound.** The transport
principal is bound on the **daemon turn** path, from the durable turn-scope
origin attribution. A **delegated** child's own loop carries no turn identity,
so it resolves no principal of its own; its write stays unattributed —
**refused** on a team daemon (the fail-closed direction, and the point of the
milestone) and attributed to the local fallback on a single-user daemon.

Binding the dispatching turn's principal into the delegated child requires
`src/scheduler/executors.rs` to carry it on `SubAgentRequest`. That file is
owned by the C007 corrective, and this milestone did not enlarge itself to
reach it. Severity: medium. The safe direction holds — the delegated child's
write is refused, not mis-attributed.

**U2 — the `AuthorizationService` seam is not wired.** The
`Capability::FileModify` check is still skipped, so `local_owner`'s
`PolicyKind::LocalOwnerBroad` — which authorises any capability on any
project — is not yet narrowed. The team-policy refusal carries the milestone
in the meantime. Wiring the seam was explicitly out of scope. Severity: low
for this milestone; it is a separate decision.

**U3 — the whole-tree verification gap opened by the disk emergency is closed.**
The host's disk reached 100% during this session (the shared XDG trash had
accumulated 287 GB of already-trashed build caches; purging it was necessary to
make the tree buildable), and the scoped tests and the guards were prioritised
over the whole-tree gates. Both gates have since been run on this tree:

- `scripts/verify.sh quick` — **green**, 86 steps: `cargo fmt --check --all`,
  every guard with its `--self-test`, and
  `cargo check --workspace --all-targets --locked`;
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — **green**
  after four lints the new code introduced were fixed.

Severity: **none**. The finding is kept in the record because the gap was real
when this milestone was written, and a reader comparing the record with the
tree's history needs to see that it was closed by measurement rather than by
assumption.

## 11. Roadmap disposition

The agent workstream M005 writer-containment corrective line moves from
`C001–C006 closed (C006 conditionally, its condition now C007's closure); C007–C009 ready` to
`C001–C009 closed`.

**C007, C008 and C009 closed in the same commit.** The line's three corrective
milestones were independent of one another, so they were implemented and
closed together rather than in sequence; the registry therefore records the
line's final state once, and each closure record names the transition the
line made in that commit.

**This is the last milestone on the agent workstream M005 writer-containment corrective line.** C007 discharged M005's own condition; C008 and C009 closed the two remaining defects the 2026-10-23 review found on this line.

The **agent workstream blackboard** parent roadmap's M005 status moves from
*conditionally closed* to **closed strictly**, because the condition — C007's
closure — is discharged.

## 12. Registry updates

`plans/registry.md` records, in this same commit:

- the agent workstream M005 writer-containment corrective row's status cell →
  `C001–C009 closed`;
- the agent workstream blackboard parent row's status cell → `M005 closed`;
- the M005 row's status cell in the closure table → `closed` (from
  `conditionally closed`), with the condition named as C007's closure;
- the "Dependency-ready implementation plans" entry for this plan → moved out
  of dependency-ready, with its closure record named;
- a new closure-table row for `C009` on the agent workstream M005
  writer-containment corrective line.

## 13. Handoff notes

The next reader should know three things.

**The commit tool was a containment illusion.** The sentinel workdir resolved
to a path *lexically under* the checkout root, so the fence's containment check
passed — and the damage was done only later, in quarantine. The fix is the
fail-closed wrapper the other fourteen tools already had.

**The team-policy refusal was dead code, and it is now reachable.** The
derivation reads durable state at fence time, so it survives a restart and
follows a membership change. The failure arm refuses, deliberately: reading an
unreadable team state as "single-user" would restore the synthetic attribution
this milestone removes.

**The delegated child's principal is the named open item.** U1 is the one gap
this milestone leaves, and it points at `src/scheduler/executors.rs` — the file
the C007 corrective owns. The next reader who picks that up has a concrete
target: carry the dispatching turn's principal on `SubAgentRequest`.

## 14. Amendment: implementation review

An implementation review of this milestone's change set found three fail-open
paths that the verification in §4 did not reach. The record is amended rather
than replaced: the closure stands, and the amendments below are what the
review changed.

**A1 — the managed-checkout probe failed open on a store error.** The probe
that decides whether a run holds a managed checkout read the run store and
answered "unmanaged" when the read failed, so the worker substituted a
containment-only fence — a fence that can never take the writer permit — for a
run that may in fact hold the group's shared checkout. That is the
anonymous-writer hole this milestone exists to close.

The probe now treats an unreadable run record as **managed** — the refusal.
An absent record is still "not managed": a run with no durable record is not
running. This matches the team-policy probe in the same change set, which
already withheld its fallback on an unreadable store.

**A2 — the probe's scope was corrected.** The first fix probed **both** the
request's run id and its parent's, on the reasoning that a delegated run's
own record may carry no worktree while its parent's does. That was wrong,
and an independent review of the fix pass caught it: a delegated child of an
isolated (mutating) parent holds no checkout of its own, so reading the
parent's checkout as the child's refused the ordinary "a mutating agent
spawns a read-only explorer" pattern — a run that can never write.

The probe answers a question about **this** run, so the run's **own**
durable record is authoritative; the parent's record is a fallback for a
request that carries no run id at all. The review's own check confirms the
distinction is the one that matters: in the scheduler path the child is
dispatched with its own run id and the dispatching run as its parent, so
probing both is what refused the child. `a_delegated_child_is_judged_by_its_own_record_not_its_parents`
covers it.

**A3 — the fence's durable authority reads were answered with "absent".**
Three reads discarded their error, and each turned a store failure into a
permissive answer:

- `DaemonWriterFenceFactory::fence_for` read the run record with
  `.ok().flatten()`. A store failure became "this run holds no checkout", so
  the caller received **no fence at all** and the write proceeded with no
  mediation. The error is now `WriterFenceError::Unavailable`.
- `DaemonWriterFence::shared_group` read the worktree record and the group's
  membership with `.ok()?`. A failed read collapsed into "this checkout is
  private", so the shared profile never came into force: no writer permit was
  taken and no principal was required, while the sibling that did hold the
  permit kept writing. The reads now fail closed, as the same file's
  `shares_live_group` already did.
- `DaemonWriterFence::worktree_id` read the run record with
  `if let Ok(Some(record))`. An unreadable record fell through to the scope's
  own identifier, so a store failure quietly answered a different question
  than the one asked.

**A4 — the guard for this milestone grew a rule.** The new
`fail-closed-authority-read` rule in `scripts/check_writer_fence_completeness.py`
asserts that these reads surface their errors. It reports **6 findings on the
baseline commit `f1b83a7a7`** — the three discarded reads, the discarded
`worktree_id` read, and the two probe arms — and **0** on the fixed tree. Its
`--self-test` grew from 11 cases to **18**: each of the seven new cases
injects one of the discarded-read forms and requires the detector to reject
it, and its paired case requires the surfaced form to be accepted. The rule
also reports a **missing** authority function as "cannot prove it fails
closed", so renaming or moving one of them is a finding rather than a silent
pass.

**A5 — the guard set changed, and the step count did not.** The review
removed a duplicated `check_writer_fence_completeness` invocation from
`scripts/verify.sh` (the guard ran twice in `run_guards`, −2 steps) and wired
`check_daemon_cwd_usage.py`, which the sibling C008 corrective extended but
which was wired into **neither** `verify.sh quick` nor CI (+2 steps). The
count is therefore unchanged at **86 steps**, over a different guard set.
