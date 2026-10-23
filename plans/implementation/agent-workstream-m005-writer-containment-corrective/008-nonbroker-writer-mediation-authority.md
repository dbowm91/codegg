# M005 Writer-Containment Corrective C008 — Non-Broker Writer Mediation Authority Completeness

Status: ready for handoff
Repository baseline: `f1b83a7a7` (`origin/main`, 2026-10-23)
Source roadmap: `plans/subsystems/agent-workstream-blackboard-m005-writer-containment-corrective-addendum.md`
Long-term requirements: `plans/000-long-term-specification.md` §§9, 16–17, 26–29; `plans/001-terminology-and-domain-model.md` §§5, 7–8
Applicable ADRs: ADR-0002 (agent run worktree isolation and control, accepted), ADR-0032 (agent workstream coordination and shared checkout ownership, accepted), ADR-0047 (restricted agent shared checkout writer contract, accepted option B)
Primary class: invariant

## 1. Objective

Make one claim true end to end:

> **A checkout that a live agent-run group holds is never written by a writer that does not hold the checkout's writer permit — and the authority that decides this cannot be defeated by the size of the catalog, by the shape of an identifier, by a missing daemon, or by the order in which the gate and the write happen.**

This milestone closes five defects found in the 2026-10-23 implementation review of the blackboard line. One is a major: the reservation authority's path lookup is a **bounded list**, so past a threshold it reports a reserved checkout as unreserved and the gate admits the writer. The other four are minors that break the claim in narrower configurations.

The claim is about the **mediation gate** — the authority in front of the fourteen non-broker writer sinks that C004 brought under the fence. The agent-loop fence and the permit store are not reopened here.

## 2. Why this milestone is ready

- **C004 is closed.** `plans/closure/agent-workstream-m005-writer-containment-corrective/004-status.md` brought every non-broker project-root writer under the reservation gate. This milestone corrects that gate's authority, not its scope.
- **Every defect below is in production code with a live call path.** The daemon's `mediate_checkout_write`, the HTTP routes, the reservation authority and the workspace registry are all exercised in the tree.
- **The defects were established by code tracing against real callers**, including the durable store's own query builder and the daemon's request entry point.
- **No new architecture is required.** Every fix is a change to an existing decision point inside the C004 contract. No ADR is needed.

## 3. Current implementation evidence

### 3.1 The reservation lookup is a bounded, oldest-first list (major)

`crates/codegg-core/src/checkout_reservation.rs:190-217` resolves the managed checkout that contains a path by listing every managed worktree:

```rust
let records = self
    .worktrees
    .list(WorktreeQuery::default())
    .await
    .map_err(|error| CheckoutReservationError::Lookup(error.to_string()))?;
```

`WorktreeQuery` derives `Default`, so it carries no workspace, repository or owner filter and `include_removed: false` (`crates/codegg-core/src/worktree_service.rs:175-181`). The SQLite store then applies a hard cap:

```rust
if !query.include_removed {
    builder.push(" AND state <> 'removed'");
}
builder
    .push(" ORDER BY created_at LIMIT ")
    .push_bind(MAX_WORKTREE_LIST as i64);
```

(`crates/codegg-core/src/worktree_service.rs:689-694`, with `MAX_WORKTREE_LIST = 512` at `:29`.)

Only the **512 oldest** non-removed rows are returned. Archived rows never leave that set — `archive` transitions to `ManagedWorktreeState::Archived` (`crates/codegg-core/src/worktree_service.rs:1268`) and only `cleanup` transitions to `Removed` (`:1339`), while the list excludes only `'removed'`.

**The catalog is user-scoped.** The durable store is built over the daemon pool against `DaemonPaths::default().data_root()` (`src/core/runtime_deps.rs:439-443`), and that default is `~/.local/share/codegg/` — `$XDG_DATA_HOME/codegg/` or `~/.local/share/codegg/` (`crates/codegg-core/src/storage/paths.rs:40-46`). **One database holds every project the user has ever opened**, and every `NewManagedCheckout` launch adds a row.

**Trigger:** 513 or more non-removed `managed_worktree` rows in the user catalog.

**Impact.** The 512 oldest rows come back; a newly created, currently reserved checkout is **absent**. `record_for_path` then returns `None`, `reservation_for_path` returns `Ok(CheckoutReservation::Unreserved)` (`crates/codegg-core/src/checkout_reservation.rs:148-150`), and the daemon's mediation admits the writer (`src/core/daemon.rs:1143-1145`). The write lands inside a checkout a live group holds, and **no `checkout_writer_refusal` row is written** — the failure is silent as well as unsafe.

**The same lookup backs process admission.** `crates/codegg-core/src/process_admission.rs:459` calls `reservation_for_path(scope_root)`, so `admit_scope` fails open in exactly the same way.

**What is not affected.** The by-id paths are sound: `reservation_for_worktree` (`crates/codegg-core/src/checkout_reservation.rs:155-166`) reads a single row, `admit_worktree` does the same, and the writer fence resolves its checkout by id (`src/core/writer_fence.rs:542-546`). The defect is specific to **path-based** resolution, which is how all fourteen mediated sinks resolve their target.

### 3.2 The HTTP mediation is skipped when the server has no daemon (minor)

`src/server/routes/file.rs:236` and `:292`, and `src/server/routes/workspace.rs:135`, gate on the daemon's presence:

```rust
if let Some(daemon) = state.daemon.as_ref() {
    daemon
        .mediate_http_checkout_write(…)
        .await
        .map_err(|conflict| AppError::Storage(StorageError::Conflict(conflict.to_string())))?;
}
let full = sanitize_path_from_root(&context.workspace_root, &req.path)?;
```

With no daemon, the write proceeds with no reservation check and no deny event.

**Trigger:** `codegg server --standalone-core` reaching `run_server(host, port, None)` — the project-local store init failure path (`src/main.rs:4806`) or the agent-resolution failure path (`:4821`).

### 3.3 The gate and the effect resolve the workspace id by different rules (minor)

The gate resolves the target through `WorkspaceId::parse`:

```rust
async fn workspace_root_for(&self, workspace_id: &str) -> Option<std::path::PathBuf> {
    let id = codegg_core::workspace::WorkspaceId::parse(workspace_id).ok()?;
    …
}
```

(`src/core/daemon.rs:1012`.)

The effect paths resolve the same identifier with `WorkspaceId::new_unchecked` — `src/core/daemon_goals.rs:719`, `:759`, `:804`, `:854`, `:905`, `:955`, `:1008`.

An identifier that fails `parse` makes `workspace_root_for` return `None`, so `checkout_write_target` returns `None` and the request is **not mediated at all** (`src/core/daemon.rs:1021-1022`, `:1111-1113`) — while the effect path still resolves the workspace and writes.

**Not currently reachable:** every producer mints a UUID (`crates/codegg-core/src/workspace.rs:527`, `project_storage.rs:635`). The defect is that the gate is **stricter** than the effect, which is the wrong direction for a fail-closed gate.

### 3.4 The reservation authority resolves against the process working directory (minor)

`crates/codegg-core/src/checkout_reservation.rs:260-270`:

```rust
pub fn canonical_identity(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(path)
    };
    …
}
```

`std::env::current_dir()` in workspace-bound daemon code is the mis-targeting class AGENTS.md forbids: the daemon's working directory is wherever it was launched, so a relative path would be resolved against an unrelated tree. `scripts/check_daemon_cwd_usage.py` scans `src/tool`, `src/scheduler` and `src/core` (`SCANNED_TREES`) and **contains no reference to `codegg-core` at all**, so this occurrence is unguarded.

**Latent:** the current inputs are absolute — `workspace.canonical_root` and `WorktreeRecord.path` under the daemon data root.

### 3.5 The gate's answer is taken before the repository lock is held (minor)

The gate runs at `src/core/daemon.rs:4287`; the repository lock is acquired later, inside the effect handlers — `src/core/daemon_documents.rs:750-753`, `src/core/daemon_project_init.rs:205-211`, `src/core/daemon_goals.rs:1043-1051`.

The check-to-write window is therefore the **lock wait**, not the instruction gap. C004's closure record §10 item 5 discloses the general time-of-check/time-of-use window; the lock-wait amplification is not recorded.

## 4. Invariants that must not regress

1. **The gate runs after authorization and before dispatch**, so a refused writer has no effect at all. Every one of the fourteen mediated sinks keeps its current ordering.
2. **A refusal is durable.** Every refusal path emits `AuditAction::CheckoutWriterRefusal` (`src/core/daemon.rs:1136`, `:1153`, `src/core/checkout_mediation.rs:230`).
3. **The target is resolved from durable identities**, never from a path in a request payload (`src/core/daemon.rs:1015-1022`).
4. **The authority fails closed.** A missing record, a lookup error, or a store failure refuses the write.
5. **The reservation derives from durable state**: the managed worktree record, the lease owner, and the live (non-terminal) agent-run group (`crates/codegg-core/src/checkout_reservation.rs:220-250`).
6. **The `.codegg/` project state and the legacy project-local database keep their recorded dispositions** in `docs/writer-disposition.toml` — `shared-sidecar` and `compatibility-store` respectively. Neither may be reclassified to "outside the checkout" without new evidence.
7. **The writer fence's by-id resolution is not weakened.** §3.1's fix changes path resolution only.

## 5. Scope

**In scope**

- The reservation authority's path resolution and the store query it depends on.
- The HTTP mediation's behaviour when no daemon is present.
- The workspace-identifier resolution on both the gate and the effect side.
- The relative-path handling in the reservation authority, and the cwd guard's coverage.
- The gate's position relative to the repository lock.
- Regression tests for each defect, and the guards in §10.

**Out of scope**

- The fourteen mediated sinks' identity and their effect ordering relative to each other. C004 brought them under the gate; this milestone changes the gate's answer, not the sink list.
- The agent-loop fence and its principal attribution — C009 of this line owns them.
- The shared checkout's placement and lease lifetime — C007 of this line owns them.
- The contained managed-worktree creation grant — the Linux trusted Git line's C009 owns it.
- The Git consistency contract and its guard — the Linux sandbox admission line's C005 owns them.
- Any change to the permit store's state machine.

## 6. Required production changes

### 6.1 Resolve the containing checkout without a list cap

The reservation authority must resolve the managed checkout containing a path with a **targeted query**, not with a bounded list. The query must be able to find a checkout regardless of how many managed worktrees the user's catalog holds.

Two acceptable shapes:

1. **A targeted query.** Extend the store's query surface with a lookup that filters by repository identity and/or path prefix, with its own bound that cannot hide a *newer* checkout — for example ordering the candidate set so the most specific containing root is found first and the cap applies to a set already narrowed by repository identity.
2. **An unbounded authority read.** Add a store method used **only** by the reservation authority that reads every non-removed row for the candidate repositories, with no `LIMIT`. This is acceptable because the authority is on the path of every non-broker write and correctness outranks the list bound — but it must be measured, and the measurement recorded in the closure record.

Whichever shape is chosen, the invariant is: **a list cap must never be able to hide a reserved checkout.**

The in-memory store is unbounded today and must keep the same semantics as the durable store after the change.

### 6.2 Treat a missing daemon as a refusal

`src/server/routes/file.rs:236`, `:292` and `src/server/routes/workspace.rs:135` must not silently skip the gate. When no daemon is present the route must refuse the write with a typed error, or the server must refuse to serve the route at all.

The refusal must be a **conflict**, not an authorization failure — the caller's own authority is untouched and it may retry once the group finishes — matching the daemon-side gate's disposition (`src/core/checkout_mediation.rs`).

### 6.3 Resolve the workspace identifier by one rule on both sides

The gate and the effect paths must use one identifier rule. The direction of the fix matters: the gate must never be **stricter** than the effect, because a stricter gate is a gate that silently stops mediating.

The preferred shape is to resolve the identifier the same way on both sides and to add an explicit "unresolvable ⇒ refuse" arm to the gate, so an identifier the registry cannot resolve produces a typed refusal rather than an unmediated write.

### 6.4 Refuse a relative path in the reservation authority

`crates/codegg-core/src/checkout_reservation.rs:260-270` must not resolve a relative path against the process working directory. Either:

- **Refuse** a relative path with a typed error (the narrowest change, and the one that matches AGENTS.md's rule), or
- **Thread the workspace root** into the authority so a relative path resolves against the daemon's own root rather than the process CWD.

Refusing is preferred: every current caller passes an absolute path, so the change is inert for correct callers and fails closed for the rest.

The cwd guard must be extended to cover `crates/codegg-core`, or a new guard must be added with the same non-vacuity requirement.

### 6.5 Re-check the reservation after the lock is held

The gate's answer must be re-checked after the repository lock is acquired and immediately before the write. The check that happens before the lock is a fast pre-filter; the check that happens under the lock is the one that counts.

Where the effect handlers acquire the repository lock, the mediation must be repeated at that point. The repeated check must use the same authority and the same refusal disposition as the first.

## 7. Ordered work packages

### WP1 — Targeted checkout resolution

**Intent:** the reservation authority finds the containing checkout whatever the catalog's size.

**Required changes:** §6.1.

**Acceptance evidence:** with 600 or more non-removed managed worktree rows in the catalog — including the most recently created one — a write into the newest checkout is **refused** with `checkout_reserved`, and a `checkout_writer_refusal` row is written.

### WP2 — Refuse the HTTP write when there is no daemon

**Intent:** the HTTP sinks are never ungated.

**Required changes:** §6.2.

**Acceptance evidence:** `POST /file`, `DELETE /file` and `POST /workspace` with no daemon present return the typed conflict and write nothing.

### WP3 — One workspace-identifier rule

**Intent:** the gate mediates exactly the writes the effect paths perform.

**Required changes:** §6.3.

**Acceptance evidence:** an identifier that the registry cannot resolve produces a typed refusal on both the checkpoint-restore and the LSP-apply paths, and is never an unmediated write.

### WP4 — Refuse a relative path; extend the cwd guard

**Intent:** the reservation authority never consults the process working directory.

**Required changes:** §6.4.

**Acceptance evidence:** a relative path is refused with a typed error; the cwd guard covers `crates/codegg-core` and its `--self-test` rejects an injected `std::env::current_dir()`.

### WP5 — Re-check the reservation under the lock

**Intent:** the window between the gate and the write is the instruction, not the lock wait.

**Required changes:** §6.5.

**Acceptance evidence:** a write that waits on the repository lock re-checks the reservation after the lock is held and is refused if the checkout became reserved in the meantime.

### WP6 — Guards and regression suite

**Intent:** the defects cannot recur silently.

**Required changes:** §10.

**Acceptance evidence:** every guard is wired into `scripts/verify.sh quick` or CI, and each has a `--self-test` case that must be rejected.

## 8. Failure, cancellation, restart and contention semantics

- **The store is unavailable during a mediation.** The authority cannot answer, so it refuses (`crates/codegg-core/src/checkout_reservation.rs:159-166` maps a lookup error to a refusal). This must hold for the new targeted query too.
- **The daemon restarts with a live group holding a checkout.** The reservation derives from durable state, so the gate's answer is unchanged across a restart. §6.1's change must not make the answer restart-dependent.
- **Two writers contend for the same reserved checkout.** The first is refused with `checkout_reserved`; the second is refused identically. Neither is admitted by a race.
- **The repository lock is held for a long time.** The re-check under the lock (§6.5) is what decides; the pre-lock check is only a fast path.
- **A group finishes while a writer waits.** The writer's re-check under the lock sees the group as terminal and the checkout is no longer reserved; the write proceeds. This is the intended behaviour and must be covered by a test so the re-check is not mistaken for a permanent refusal.

## 9. Compatibility and migration

- **No schema change is expected.** The targeted query reads the existing `managed_worktree` table. If the chosen query shape needs an index, it must be an additive migration with the schema-bound and index conventions the roadmap's cross-cutting section requires, and the migration's version must be recorded.
- **The durable records are unchanged.** No row's shape changes; only the query that finds them does.
- **The HTTP refusal changes an API surface.** A `POST /file` that previously succeeded with no daemon now returns a conflict. This is the intended correction — the write was ungated — and must be recorded in the closure record and in `docs/` if the route's documented behaviour changes.
- **The workspace-identifier rule change** must not invalidate stored records. Every stored identifier is a UUID today, so the rule change is inert for existing data; §6.3 only changes how an unparseable identifier is disposed of.
- **The cwd-guard extension** is a check-coverage change. It may reject pre-existing occurrences elsewhere in `crates/codegg-core`; each must be dispositioned individually, not exempted wholesale.

## 10. Required tests

**Focused unit tests**

- `crates/codegg-core/src/checkout_reservation.rs` — a relative path is refused; an absolute path still resolves; the deepest containing root still wins.
- `crates/codegg-core/src/worktree_service.rs` — the targeted lookup returns a checkout that the capped list would have hidden.

**Integration tests**

1. `a_reserved_checkout_is_refused_past_the_list_cap` — seed 600 non-removed managed worktree rows, reserve the newest checkout, and assert the write is refused with `checkout_reserved` and that a `checkout_writer_refusal` row is written.
2. `the_http_file_write_is_refused_without_a_daemon` — and the same for `DELETE /file` and `POST /workspace`.
3. `an_unresolvable_workspace_identifier_is_refused_not_unmediated` — on the checkpoint-restore and LSP-apply paths.
4. `a_relative_path_is_refused_by_the_reservation_authority`.
5. `a_write_that_waits_on_the_repository_lock_re_checks_the_reservation` — and its counterpart, `a_write_proceeds_when_the_group_finishes_while_it_waits`.
6. The fourteen existing per-sink ordering tests remain green — no sink's gate/effect ordering changes.

**Static guards** (new, each with a `--self-test`)

- A guard asserting the reservation authority's path resolution does not depend on a capped list — for example, that the authority's query path does not go through the capped `list` surface.
- A guard asserting the HTTP mediation's refusal is not conditional on the daemon's presence — that an `if let Some(daemon)` form may not silently skip the gate.
- A guard asserting the gate and the effect use the same workspace-identifier rule.
- An extension of `scripts/check_daemon_cwd_usage.py` covering `crates/codegg-core`.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=2 bash scripts/verify.sh quick
CARGO_BUILD_JOBS=2 cargo test --locked -p codegg-core --test writer_permit
CARGO_BUILD_JOBS=2 cargo test --locked -p codegg --test shared_checkout_c006
python3 scripts/check_daemon_cwd_usage.py --self-test
python3 scripts/check_writer_disposition.py --self-test
```

Never run `--all-features` for a workspace sweep.

## 12. Documentation updates

- `plans/subsystems/agent-workstream-blackboard-m005-writer-containment-corrective-addendum.md` — the milestone table gains C008; the status line records the open corrective work.
- `architecture/writer_permit.md` — the documented reservation authority and its lookup must describe the new resolution, and the list cap's role (or its removal) must be stated.
- `architecture/blackboard_contract_matrix.md` — if the matrix names the reservation lookup, it must name the targeted resolution.
- `docs/security-semantics.md` — the HTTP refusal with no daemon is a user-visible behaviour change and must be recorded there if the document describes the file routes.
- `plans/closure/agent-workstream-m005-writer-containment-corrective/004-status.md` — **read-only**. C004's closure disclosed the general TOCTOU; §3.5 records the lock-wait amplification. The correction goes in this milestone's closure record, not by editing C004's record.

## 13. Acceptance criteria

1. With 600 or more non-removed managed worktree rows in the user catalog, a write into a checkout a live group holds is refused, and a `checkout_writer_refusal` row is written.
2. The same refusal holds for process admission through `admit_scope`.
3. `POST /file`, `DELETE /file` and `POST /workspace` never write without the mediation gate in force.
4. An identifier the workspace registry cannot resolve produces a typed refusal on both the gate and the effect side, never an unmediated write.
5. The reservation authority never resolves a path against the process working directory, and the cwd guard covers `crates/codegg-core`.
6. A write that waits on the repository lock re-checks the reservation after the lock is held.
7. Every defect above has a regression test that fails against the baseline and passes after the fix, and every new guard is wired in with a `--self-test` case that is rejected when the defect is reintroduced.
8. `verify.sh quick`, `cargo clippy --workspace --all-targets -- -D warnings` and the named test targets are green.

## 14. Stop conditions

Report rather than improvise if:

- **§6.1's targeted query requires a schema migration** the roadmap's cross-cutting storage conventions do not already permit. Report the required shape first.
- **The targeted query cannot avoid reading a large candidate set** in a way that is acceptable for the mediation path's latency. Measure first; report the measurement. A correct-but-impractical authority is a decision for the maintainer, not a silent trade.
- **§6.2's refusal changes an HTTP contract** that a client depends on in a way the route's documentation does not already describe. Report the client impact.
- **§6.3's single identifier rule requires the store's records to be migrated** — for example if non-UUID identifiers are found in an existing catalog. Report the population before changing the rule.
- **§6.5's under-lock re-check requires a lock-ordering change** that could deadlock against the reservation authority's own locks. Report the ordering before implementing.
- **Extending the cwd guard to `crates/codegg-core` surfaces pre-existing occurrences** whose disposition is not obvious. Report them individually.

## 15. Closure evidence required

1. A closure record at `plans/closure/agent-workstream-m005-writer-containment-corrective/008-status.md`, with the sixteen-section structure the closure template requires.
2. A requirement-to-evidence matrix mapping each of §13's eight criteria to the test or command that demonstrates it.
3. The exact verification commands and their results, labelled honestly as local or hosted.
4. A measured statement of the §6.1 lookup's cost at the catalog sizes the milestone tested, with the numbers shown.
5. An invariant review confirming §4's seven invariants are intact.
6. A registry update in the same commit as the closure.
7. A disposition for C004's §10 TOCTOU item, recording the lock-wait amplification this milestone closes.

## 16. Handoff notes

- The five defects were found by a 2026-10-23 review of `origin/main` at `f1b83a7a7`, by code tracing against the real callers. They are not reproduced by a failing test, because none exists.
- The tree at `f1b83a7a7` is **green**. `codegg-core --test writer_permit` (21), `codegg --test shared_checkout_c006` (16), `codegg --test contained_worktree_c008` (5), `codegg --test composed_two_agent_race_m011` (3) and `codegg --lib tool::writer_fence` (3) all pass. These defects are latent. Do not treat the green suite as evidence against §3.
- §3.1 is the highest-value item: it is the only defect that makes the gate admit a writer into a live group's checkout, and it does so **silently** — no refusal row is written. Start with WP1.
- §3.1's threshold is 512 managed worktrees in one user-scoped database. That is reachable for an active user across many projects, and it grows without bound because archived rows never leave the set.
- C007 and C009 of this line are the sibling corrective milestones for shared-checkout placement and the agent-loop fence. They are independent and must not be folded in.
- This milestone does not enable the WorkOrder `shared`/`serialized` mutation policy, which still fails closed.
