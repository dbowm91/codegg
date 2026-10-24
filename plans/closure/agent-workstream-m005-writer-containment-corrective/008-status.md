# Agent Workstream M005 Writer-Containment Corrective C008 — Non-Broker Writer Mediation Authority Completeness: Status

Status: closed

Milestone: `plans/implementation/agent-workstream-m005-writer-containment-corrective/008-nonbroker-writer-mediation-authority.md`
Review baseline: `f1b83a7a7` (`origin/main`, 2026-10-23) — the tree §3's evidence was gathered against
Implementation baseline: `f2639b5d2` (`origin/main`, 2026-10-23) — the tree this work was applied to

## 1. Executive finding

**The reservation authority could fail open.** `CheckoutReservationService` resolved the managed checkout containing a path by listing **every** managed worktree and filtering in memory:

```rust
let records = self.worktrees.list(WorktreeQuery::default()).await
    .map_err(|error| CheckoutReservationError::Lookup(error.to_string()))?;
```

`WorktreeQuery::default()` carries no filter, and the SQLite store applies a
hard cap: `ORDER BY created_at LIMIT MAX_WORKTREE_LIST`, where
`MAX_WORKTREE_LIST = 512`. The catalog is **user-scoped** — one SQLite
database under `~/.local/share/codegg/` holds every project the user has ever
opened — and archived rows never leave the set (only cleanup moves a row to
`removed`, which the query excludes).

So with 513 or more non-removed rows, the 512 **oldest** come back and a newly
created, currently **reserved** checkout is simply absent. `record_for_path`
returned `None`, `reservation_for_path` returned `Ok(Unreserved)`, and the
daemon's mediation **admitted the writer** — silently, with **no
`checkout_writer_refusal` row** to show that a gate had been passed. The same
lookup backed process admission.

The fix is structural rather than a bigger limit: the authority now reads
through an **unbounded** candidate surface, `WorktreeStore::list_authority_candidates`,
which has no `LIMIT`. A cap can no longer hide a reserved checkout.

Four smaller defects closed with it: the HTTP file and workspace routes
**skipped the mediation gate entirely** when no daemon was attached; the gate
and the effect resolved the workspace identifier by **different rules**, so an
identifier the gate could not parse made the gate return `None` — not
mediated — while the effect still wrote; the reservation authority resolved a
relative path against the **process working directory**; and the gate's answer
was taken **before** the repository lock was held, so the window was the lock
wait.

## 2. Requirement-to-evidence matrix

| §13 criterion | Evidence | Result |
|---|---|---|
| 1. With 600+ non-removed rows, a write into a checkout a live group holds is refused, and a `checkout_writer_refusal` row is written | §4.1 | **met** |
| 2. The same refusal holds for process admission through `admit_scope` | §4.1 | **met** |
| 3. `POST /file`, `DELETE /file` and `POST /workspace` never write without the mediation gate in force | §4.2 | **met** |
| 4. An identifier the workspace registry cannot resolve produces a typed refusal on both the gate and the effect side, never an unmediated write | §4.3 | **met** |
| 5. The reservation authority never resolves a path against the process working directory, and the cwd guard covers `crates/codegg-core` | §3.4 and §5 | **met** |
| 6. A write that waits on the repository lock re-checks the reservation after the lock is held | §3.5 and §4.4 | **met** |
| 7. Every defect above has a regression test, and every new guard is wired in with a `--self-test` case that is rejected when the defect is reintroduced | §4 and §5 | **met** |
| 8. `verify.sh quick`, workspace clippy and the named test targets are green | §4.6 and §4.7 | **met, except for one measured pre-existing flake** — `verify.sh quick` is green across its 86 steps at the closing commit and the workspace clippy with `-D warnings` is green; `core_daemon` and `shared_checkout_c006` are green, and `server_feature` reports 100 passed / 1 failed on a **pre-existing** flake that was measured on the untouched baseline — see §10 (U1) |

## 3. Production implementation evidence

### 3.1 The unbounded authority read

`crates/codegg-core/src/worktree_service.rs` gained
`WorktreeStore::list_authority_candidates`:

- **SQLite store** — `SELECT … FROM managed_worktree WHERE state <> 'removed'
  ORDER BY created_at`, with **no `LIMIT`**.
- **In-memory store** — every non-removed row, sorted by `created_at`. It was
  already unbounded; it now says so explicitly and keeps the durable store's
  semantics.

The `list` surface is unchanged and is now **documented as a display bound**:
`MAX_WORKTREE_LIST` is a presentation cap, and the constant's own doc comment
says so and names the authority read that replaces it.

`CheckoutReservationService` now resolves through the authority read, so the
cap cannot hide a reserved checkout.

**Measured cost.** The authority read is a single indexed query over the
non-removed rows of one user's catalog. At the catalog sizes the milestone
tested — 601 non-removed rows, of which one is the reserved checkout — the
resolution is a single `fetch_all` and the whole test's reservation phase
completes in well under a second. The point of the change is not speed: it is
that the *result* is now correct at any catalog size, where before it was
correct only below 513.

### 3.2 The HTTP routes refuse without a daemon

`src/server/routes/file.rs` and `src/server/routes/workspace.rs` — the gate
was conditional on the daemon's presence:

```rust
if let Some(daemon) = state.daemon.as_ref() { … }
```

With no daemon the write proceeded ungated. The routes now **refuse** with a
typed error when no daemon is attached. The refusal is a **conflict**, not an
authorization failure — the same disposition the daemon-side gate gives a
reserved checkout — because the request was well-formed and the checkout was
merely not adjudicable.

### 3.3 One workspace-identifier rule on both sides

`src/core/daemon.rs` and `src/core/daemon_goals.rs` — the gate used
`WorkspaceId::parse` and the effect paths used `WorkspaceId::new_unchecked`.
An identifier that failed `parse` made the gate return `None` — **not
mediated** — while the effect still wrote.

Both sides now use one rule, and the gate has an explicit **unresolvable →
refuse** arm, so an identifier the registry cannot resolve produces a typed
refusal rather than an unmediated write. The gate is never stricter than the
effect and never laxer than it.

### 3.4 The authority never resolves against the process working directory

`crates/codegg-core/src/checkout_reservation.rs` — `canonical_identity` called
`std::env::current_dir()` for a relative path. AGENTS.md forbids this in
workspace-bound daemon code, and `scripts/check_daemon_cwd_usage.py` scanned
`src/tool`, `src/scheduler` and `src/core` — **not** `crates/codegg-core` at
all, which is why the violation was invisible.

The authority now has `canonical_identity_absolute`, which **refuses** a
relative path with a typed error (`CheckoutReservationError::RelativePath`).
Every current caller passes an absolute path, so the refusal is inert for
correct callers and fails closed for the rest.

`scripts/check_daemon_cwd_usage.py` now covers `crates/codegg-core`, with
three new self-test cases: a process-cwd read in the core crate is caught, a
bare `current_dir()` is caught too, and a core-crate **test module** is not a
violation.

### 3.5 The re-check runs under the lock

The gate's answer was taken before the repository lock was acquired, so the
window was the **lock wait**, not the instruction gap: a writer that queued
behind another writer could find the checkout reserved by the time its turn
came.

The pre-lock gate stays as a fast pre-filter. The re-check now runs **after**
the lock is held, immediately before the write, using the same authority and
the same refusal disposition:

- `src/core/daemon_documents.rs` — document create, delete and save;
- `src/core/daemon_project_init.rs` — the `AGENTS.md` publish;
- `src/core/daemon_goals.rs` — checkpoint undo, undo-latest, reapply and
  reapply-latest;
- `src/lsp/mutation.rs` — the **LSP preview apply**.

The LSP leg needed its own shape: the applier takes the repository lock
itself, so the handler cannot hold the lock and hand it over — the applier
would wait on a mutex it already holds. The applier therefore takes an
`UnderLockReservationCheck` and calls it **while it holds the lock**; the
daemon supplies the implementation, because the daemon owns the authority and
the refusal context that a refusal row needs. The refusal reaches the
transport as `code: "checkout_reserved"`, the same disposition every other
mediated sink gives it.

## 4. Verification executed

All commands were run **locally**. No hosted lane was consulted, and none is claimed.

### 4.1 The capped catalog

`tests/core_daemon/checkout_mediation_authority_c008.inc` gained
`a_reserved_checkout_is_refused_past_the_list_cap`: the catalog is filled past
the cap with **older** ordinary rows, so the reserved checkout is the
**newest** — exactly the row the capped list would hide. The write is refused
and a `checkout_writer_refusal` row is written.

Against the baseline, the same tree **admitted** the write: the capped list
returned the 512 oldest rows, the reserved checkout was absent, the authority
answered `Unreserved`, and no refusal row was written.

`admit_scope` — the process-admission path over the same lookup — is refused
the same way.

### 4.2 The HTTP routes

`tests/server_feature/checkout_mediation_authority_c008_http.inc` gained
three tests, one per route:

- `the_http_file_write_is_refused_without_a_daemon`;
- `the_http_file_delete_is_refused_without_a_daemon`;
- `the_http_workspace_create_is_refused_without_a_daemon`.

Each builds a `ServerState` with no daemon attached and asserts the route
refuses rather than writing.

### 4.3 The unresolvable identifier

`an_unresolvable_workspace_identifier_is_refused_not_unmediated` — an
identifier the workspace registry cannot resolve produces a typed refusal on
the gate side and on the effect side, and no write.

### 4.4 The lock wait

Two tests, in the same `.inc`:

- `a_write_that_waits_on_the_repository_lock_re_checks_the_reservation` — the
  writer queues behind a held repository lock; when its turn comes the
  reservation is re-checked and the write is refused.
- `a_write_proceeds_when_the_group_finishes_while_it_waits` — the
  counterpart: the group finishes while the writer waits, and the write
  proceeds.

### 4.5 The relative path

`a_relative_path_is_refused_by_the_reservation_authority`.

### 4.6 The named test targets

| Target | Result |
|---|---|
| `cargo test -p codegg --features server --test core_daemon` | **507 passed, 0 failed** —
  **498** without the `server` feature, which gates the nine
  `work_orders_m005_trigger::http_fire` tests |
| `cargo test -p codegg --test server_feature` | **100 passed, 1 failed** — the failure is `projection_transport_real::real_tui_pending_snapshot_disconnect_convergence_50_cycles`, a TUI projection-transport test unrelated to this milestone; it is a **pre-existing** flake, measured on the untouched baseline, and the analysis is in §10 (U1) |
| `cargo test -p codegg --test shared_checkout_c006` | **19 passed, 0 failed** |
| `cargo test -p codegg-core` | green |

### 4.7 The whole-tree gates

`scripts/verify.sh quick` is **green** on the implementation tree at the
closing commit — 86 steps,
including `cargo fmt --check --all`, every guard with its `--self-test`, and
`cargo check --workspace --all-targets --locked`. `cargo clippy --workspace
--all-targets --locked -- -D warnings` is **green** as well, after four lints
the new code introduced were fixed.

## 5. Guards

`scripts/check_checkout_mediation_authority.py` is **new**, and asserts four
things, each with a matching injection in its `--self-test`:

| Rule | Asserts | Injection |
|---|---|---|
| the reservation authority does not read through the capped list | `list_authority_candidates` is used, not `list` | a reservation authority that reads through the capped list is rejected |
| the durable authority read carries no `LIMIT` | the SQL has no `LIMIT` | a durable authority read that carries a `LIMIT` is rejected |
| the HTTP routes refuse without a daemon | no `if let Some(daemon)` skip | an HTTP route that skips the gate without a daemon is rejected |
| the gate and the effect share one workspace-identifier rule | one rule on both sides, and the gate has an explicit unresolvable arm | a gate that resolves the workspace more strictly than the effect is rejected; an effect side that resolves it more strictly is rejected; a gate with no explicit unresolvable-target arm is rejected |

```
$ python3 scripts/check_checkout_mediation_authority.py
checkout-mediation-authority check ok: the reservation authority reads an unbounded candidate set, the HTTP routes refuse without a daemon, and the gate and the effect share one workspace-identifier rule

$ python3 scripts/check_checkout_mediation_authority.py --self-test
  ok: accepted the real tree
  ok: rejected a reservation authority that reads through the capped list
  ok: rejected a durable authority read that carries a LIMIT
  ok: rejected an HTTP route that skips the gate without a daemon
  ok: rejected a workspace route that skips the gate without a daemon
  ok: rejected a gate that resolves the workspace more strictly than the effect
  ok: rejected an effect side that resolves the workspace more strictly
  ok: rejected a gate with no explicit unresolvable-target arm
checkout-mediation-authority self-test ok
```

`scripts/check_daemon_cwd_usage.py` was extended to cover `crates/codegg-core`:

```
  ok   M005 corrective C008: a process-cwd read in the core crate is caught
  ok   M005 corrective C008: a bare current_dir() in the core crate is caught too
  ok   M005 corrective C008: a core-crate test module is not a violation

self-test passed: 11 case(s).
```

Both guards run in `scripts/verify.sh quick` and in CI, each with its
`--self-test`.

## 6. Invariant review

§4's invariants, reviewed against the implementation tree:

| # | Invariant | Status |
|---|---|---|
| The reservation authority's path resolution cannot be hidden by a list cap | **restored** — the authority reads an unbounded surface |
| The by-id paths are unaffected | **intact** — `reservation_for_worktree` and `admit_worktree` are untouched |
| The gate refuses rather than admits when it cannot answer | **intact and strengthened** — an unreadable authority fails closed, and an unresolvable identifier now has its own refusal arm |
| The HTTP surface refuses with a **conflict**, not an authorization failure | **intact** — the new no-daemon refusal uses the same disposition |
| The process-admission path uses the same authority | **intact** — `admit_scope` reads the same unbounded surface |
| The daemon never resolves a path against the process working directory | **restored** — the authority refuses a relative path, and the cwd guard now covers `crates/codegg-core` |
| No lock-ordering change was needed | **intact** — the under-lock re-check takes the repository lock the write already takes, and adds no new lock |

## 7. Failure, cancellation, restart and contention semantics

**An unreadable authority fails closed.** `admit_scope` returns
`ProcessAdmission::Denied` with `ProcessDenialReason::AuthorityUnavailable`
when the reservation lookup errors.

**An unresolvable identifier fails closed.** The gate has an explicit
refusal arm, so an identifier it cannot resolve produces a typed refusal
rather than an unmediated write.

**No daemon fails closed.** The HTTP routes refuse rather than writing
ungated.

**A relative path fails closed.** The authority refuses it.

**The lock wait is closed.** The re-check runs after the lock is held. If the
group finishes while a writer waits, the write proceeds; if the checkout is
reserved when the writer's turn comes, the write is refused and a refusal row
is written.

**No lock-ordering change.** The re-check takes no new lock: it runs while the
repository lock the write already requires is held, and the reservation
authority's own read is a single query. The LSP leg was the one that needed a
new shape, and it got one — the applier calls back into the daemon while it
holds the lock, rather than the daemon holding the lock and handing it over.

## 8. Compatibility and migration review

No migration. No schema change. The new `list_authority_candidates` is a
**read** surface: it adds a method to the `WorktreeStore` trait, which has two
implementations (SQLite and in-memory), both updated. No durable row's shape
changed, and no existing query changed.

The `list` surface is deliberately unchanged: it is still the capped
oldest-first list, and it is still what a display caller gets. Only the
authority stopped using it.

The HTTP routes' behaviour changed observably: a request that previously
**succeeded** without a daemon now **refuses**. That is the correction the
milestone exists to make.

## 9. Security review

The fail-open direction is closed. Before this milestone, a user with more
than 512 non-removed managed worktrees could have a write into a reserved
checkout **admitted** — with no refusal row, so no evidence that a gate had
been passed, and no way to notice after the fact.

Three further directions are closed:

- the HTTP surface no longer writes without a gate;
- an unresolvable identifier no longer bypasses the gate;
- the authority no longer resolves a path against a working directory that is
  wherever the daemon happened to be launched — which, per AGENTS.md, is the
  bug class that silently retargets a whole session into an unrelated tree.

The new guard is wired into `verify.sh quick` and CI, so a future change that
reintroduces any of the four shapes fails the build rather than waiting for a
reviewer to notice.

## 10. Unresolved findings

**U1 — one `server_feature` test fails, and it is a pre-existing flake.**
`projection_transport_real::real_tui_pending_snapshot_disconnect_convergence_50_cycles`
fails on the implementation tree, in isolation as well as in the suite:

```
assertion `left == right` failed: every installed TUI projection forwarder must be joined
  left: 0
 right: 1
```

**It is not this milestone's defect, and this is measured rather than
asserted.** The test was built and run against the untouched baseline tree
(`f1b83a7a7`, in a separate worktree with the shared target directory) six
times. It failed three times and passed three times, with the identical
assertion:

```
baseline run 1: ok. 1 passed; 0 failed
baseline run 2: FAILED. 0 passed; 1 failed
baseline run 3: FAILED. 0 passed; 1 failed
baseline run 4: FAILED. 0 passed; 1 failed
baseline run 5: ok. 1 passed; 0 failed
baseline run 6: ok. 1 passed; 0 failed
```

The file `tests/server_feature/projection_transport_real.inc` is **byte-identical**
to the baseline on this tree (`diff` against `f1b83a7a7` reports no difference),
and nothing in this milestone touches the projection transport, the TUI
connection task probes, or the forwarder join path.

**Root cause.** The assertion compares the joined-forwarder counter with the
installed-forwarder counter without waiting for the join. The join happens in
the connection's cleanup drain (`src/server/ws.rs:2255`,
`cleanup_projection_connection_state`), which runs **after** the cleanup pass is
counted (`src/server/ws.rs:2224`) and after the lifecycle barrier is released.
The test's `wait_for_probe_baseline` waits for the cleanup counter, so it
converges while the forwarder is still un-joined, and the next assertion races
that join.

**Why no fix is filed here.** Three candidate fixes were tried and measured,
and none is filed, because each one is either wrong or makes the tree worse:

1. *Wait for the forwarder inside `wait_for_probe_baseline`.* **Wrong.** The
   forwarder cannot join before the lifecycle barrier is released, and that
   barrier is released **after** this helper returns. The wait therefore
   deadlocks and the test times out.
2. *Wait for the forwarder inside the rollback assertion helper.* **Correct in
   isolation, insufficient in the suite.** The test then passes 3/3 in
   isolation, but under full-suite parallelism the forwarder is still un-joined
   after **ten** seconds (`joined=0 installed=1`), so the forwarder is not
   merely slow to join on that path. This attempt also perturbed
   `real_core_queue_saturation_observer_records_timeout`, a second
   load-sensitive test in the same file, which then failed intermittently.
3. *A longer bound.* **No effect.** The same ten-second measurement above
   applies to the bound itself.

A correct fix therefore has to establish **when the forwarder join is
guaranteed** for a connection whose lifecycle barrier was released after the
client disconnected — a product-level question about the seam and the
connection teardown ordering, not a one-line assertion change. That belongs to
its own corrective plan; this milestone does not open it, and does not ship a
half-fix that trades one flaky test for another.

Severity: low, and **not** this milestone's defect. The failure is recorded
here so that the green results of every other target in §4.6 are read against
a known, measured, pre-existing flake rather than as an unqualified pass.

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
  after four lints the new code introduced were fixed (a needless borrow, a
  redundant closure, a `== false` comparison, and the `#[must_use]` the
  `async-trait` expansion injects, suppressed at the macro site the way the
  rest of the tree does it).

Severity: **none**. The finding is kept in the record because the gap was real
when this milestone was written, and a reader comparing the record with the
tree's history needs to see that it was closed by measurement rather than by
assumption.

## 11. Roadmap disposition

The agent workstream M005 writer-containment corrective line moves from
`C001–C006 closed (C006 conditionally, its condition now C007's closure); C007 closed; C008–C009 ready` to
`C001–C009 closed`.

**C007, C008 and C009 closed in the same commit.** The line's three corrective
milestones were independent of one another, so they were implemented and
closed together rather than in sequence; the registry therefore records the
line's final state once, and each closure record names the transition the
line made in that commit.

**This milestone does not change M005's own status.** M005's condition was
C007's closure, and C007 closed it; C008 is a separate defect on a separate
surface.

## 12. Registry updates

`plans/registry.md` records, in this same commit:

- the agent workstream M005 writer-containment corrective row's status cell →
  `C001–C009 closed`;
- the "Dependency-ready implementation plans" entry for this plan → moved out
  of dependency-ready, with its closure record named;
- a new closure-table row for `C008` on the agent workstream M005
  writer-containment corrective line.

## 13. Handoff notes

The next reader should know three things.

**The cap was never the bug; using a capped list for an authority was.** The
fix is a separate, uncapped read surface rather than a larger limit, because a
larger limit only moves the threshold. The `MAX_WORKTREE_LIST` constant's own
doc comment now says which surface it bounds and which one the authority uses.

**The gate and the effect must agree.** Two different identifier rules let an
identifier the gate could not parse skip the gate while the effect wrote. The
gate now has an explicit "cannot resolve → refuse" arm, so the two can never
disagree about whether a write is mediated.

**The LSP leg needed a different shape.** Every other sink takes the
repository lock in the daemon, so the daemon could re-check after acquiring
it. The LSP applier takes the lock itself, so it calls back into the daemon
while it holds it. That asymmetry is the reason the applier takes a trait
rather than the daemon handing the lock over.

## 14. Amendment: implementation review

An implementation review of this milestone's change set found four defects
that the verification in §4 did not reach. The record is amended rather than
replaced: the closure stands, and the amendments below are what the review
changed.

**A1 — a corrupt record row was dropped silently.** `record_for_path`
canonicalized each candidate record's path with `canonical_identity`, which
anchors a relative path at the **process's current directory**. A record
whose path was relative therefore never matched the absolute target and was
dropped from the candidate set — so a reserved checkout answered
"unreserved" and the gate admitted the writer. The record's path is now
resolved with `canonical_identity_absolute`, and an unresolved one is a typed
error the lookup fails closed on.
`a_relative_record_path_is_a_typed_error_rather_than_a_silent_skip` covers it.

**A2 — an unresolvable target reported an observed reservation.** The
`Unresolvable` arm produced a conflict with `lookup_failure: Some(..)`, but
`CheckoutConflict::code()` answered `"checkout_reserved"` for every conflict
and `recovery()` offered the reservation's text — "retry after that group
finishes" — to a caller whose group may not exist. An unresolved target now
has its own code (`checkout_target_unresolved`) and its own recovery text.
`a_lookup_failure_is_not_reported_as_an_observed_reservation` covers it.

**A3 — the LSP preview-apply sink flattened the refusal to a message.** The
applier carries the under-lock refusal as a `String`, so the handler
hardcoded `checkout_reserved` and the distinction A2 introduces never reached
that sink. The refusal now carries its code — `CheckoutRefusal` — and the
handler reports the code the daemon produced.

**A4 — the two stores could answer the same lookup differently.** The
authority read ordered by `created_at` alone, so records that share a
timestamp came back in an unspecified order — the durable store in the
database's order, the in-memory store in its insertion order. The ordering
is now a total order, broken by `worktree_id`, and the two agree.

**A5 — the guard's identifier rule was over-specified and under-scoped.**
The rule demanded that the gate and the effect use *one* identifier rule, and
it read one of the three effect families. The tree does not have that shape
and does not need it: the gate uses the most permissive rule
(`WorkspaceId::new_unchecked`) and the document and project-init effects apply
a stricter lexical check on top, which can only refuse more. The rule now
asserts the property that matters — the gate's rule must be at least as
permissive as **every** effect family's — and it reads all three families,
so one that is not read can no longer drift silently. Its `--self-test` grew
from 8 cases to 9: the new case injects an effect family the gate does not
cover resolving the workspace more permissively than the gate, and requires
the detector to reject it.

**A6 — the `verify.sh quick` step count.** The review removed a duplicated
`check_writer_fence_completeness` invocation from `scripts/verify.sh` (−2
steps) and wired `check_daemon_cwd_usage.py`, which this milestone extended
but which was wired into **neither** `verify.sh quick` nor CI (+2 steps). The
count is therefore unchanged at **86 steps** over a different guard set — the
count at the branch tip `8225f42dd`; the merged tree's count is in §14 A7.

**A7 — the merged tree's measurements.** The **86 steps** recorded above were
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

`shared_checkout_c006` is **20** in the merged tree — §4.6's table keeps the
closing-commit number, and the review pass added the lease-release regression
test. `core_daemon` is **508** with `--features server` and **499** without it,
both re-measured on the merged tree.

**A8 — the fourth reconciliation with `main`.** `origin/main` advanced again
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

**A9 — the fifth reconciliation with `main`.** `origin/main` advanced again
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
