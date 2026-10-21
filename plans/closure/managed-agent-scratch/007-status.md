# Managed Agent Scratch Milestone M007 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/managed-agent-scratch/007-scratch-retention-sweep-and-unreclaimable-tree-disposition.md`

Source subsystem roadmap:

- `plans/subsystems/managed-agent-scratch-roadmap.md#7-milestones`

Predecessor milestone:

- `plans/implementation/managed-agent-scratch/006-scratch-usage-census-and-operator-visibility.md`
  — closed; its census is this milestone's WP0 input

Repository baseline reviewed: `84aa4752` on `main` (the M006 merge)

## 1. Executive finding

M007 delivered the bounded reclamation the milestone was scoped to deliver,
and it delivered **nothing else**. The daemon now reclaims its own disposable
scratch tree on its own schedule, and the residual it cannot reclaim has an
explicit, audited, operator-initiated disposition.

The growth of a session tree that is never soft-deleted is now **bounded**.
That was the gap M006 measured and M007 closed.

Three properties carry the milestone:

- **The lease is derived from durable state that already exists.** An
  `accepted` turn admission receipt, a non-terminal agent run, or a
  non-terminal job. There is **no new lease primitive, no new table, and no
  migration**, and the decision reads a timestamp rather than a tree, so the
  sweep performs **no size traversal** and never runs on the agent hot path.
- **Each candidate is decided inside one `BEGIN IMMEDIATE` transaction.** A
  deferred transaction would take a shared read lock at its first `SELECT` and
  then have to upgrade it at its first write, which SQLite refuses without
  waiting — the C002 deadlock shape. The sweep takes the writer lock first,
  re-reads the session inside it, decides, and reclaims, all under one lock.
- **The Unix-only multiply-linked refusal is gone.** On Linux and macOS a tree
  containing a hardlink is reclaimable again, matching the Windows
  implementation and test that already had this behaviour.

**The disposable-cache contract is preserved verbatim.** "Cache contents are
disposable and may be removed by host cache maintenance" is unchanged. The
sweep is the daemon removing its own disposable cache, not a promise about
survival.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| An idle session's tree is reclaimed | `core::scratch_sweep::tests::the_sweep_reclaims_an_idle_session_tree` | pass — one tree reclaimed, and the tree is gone |
| A session inside the retention window keeps its tree | `core::scratch_sweep::tests::the_sweep_leaves_an_active_session_tree` | pass — `skipped_active` 1, the tree survives |
| An accepted turn receipt leases the tree | `core::scratch_sweep::tests::the_sweep_refuses_a_leased_session_tree` | pass — `skipped_leased` 1, the tree survives |
| A non-terminal run leases the tree | `core::scratch_sweep::tests::the_sweep_refuses_a_session_with_a_non_terminal_run` | pass — `skipped_leased` 1, the tree survives |
| A non-terminal job leases the tree | `core::scratch_sweep::tests::the_sweep_refuses_a_session_with_a_non_terminal_job` | pass — `skipped_leased` 1, the tree survives |
| A soft-deleted session's tree is reclaimed | `core::scratch_sweep::tests::the_sweep_reclaims_a_soft_deleted_session_tree` | pass — `reclaimed_deleted` 1, and it is **not** counted as an orphan |
| A deleted session whose work is live keeps its tree | `core::scratch_sweep::tests::the_sweep_refuses_a_deleted_session_with_live_work` | pass — `skipped_leased` 1 |
| A restore during a sweep is not lost | `core::scratch_sweep::tests::the_sweep_leaves_a_restored_session_alone` | pass — the writer-lock re-read sees the restore and the tree survives |
| A tree that vanished mid-sweep is recorded, not failed | `core::scratch_sweep::tests::the_sweep_records_a_tree_that_vanished` | pass — `vanished` 1, `failed` 0 |
| An orphan past the grace period is reclaimed | `core::scratch_sweep::tests::the_sweep_reclaims_an_orphan_tree_after_the_grace_period` | pass — `reclaimed_orphan` 1 |
| An orphan inside the grace period is left alone | `core::scratch_sweep::tests::the_sweep_leaves_an_orphan_inside_the_grace_period` | pass — the tree survives |
| The sweep is bounded and says which bound stopped it | `core::scratch_sweep::tests::the_sweep_records_the_bound_that_stopped_it`; `core::scratch_sweep::tests::the_sweep_records_the_reclaim_bound_that_stopped_it` | pass — `stopped` is `Candidates` and `Reclaims` respectively |
| A disabled policy never fires | `core::scratch_sweep::tests::a_disabled_policy_never_sweeps` | pass — `candidates` 0 |
| The operator disposition is a dry run by default | `core::scratch_sweep::tests::the_operator_reclaim_is_a_dry_run_by_default` | pass — the tree survives, and the outcome reports 2 entries and 96 bytes |
| The operator disposition removes the tree when applied | `core::scratch_sweep::tests::the_operator_reclaim_removes_the_tree_when_applied` | pass — the tree is gone |
| The operator disposition refuses a leased tree | `core::scratch_sweep::tests::the_operator_reclaim_refuses_a_leased_tree` | pass — `leased` true, the tree survives |
| The operator disposition of an unknown digest is a clean no-op | `core::scratch_sweep::tests::the_operator_reclaim_of_an_unknown_digest_is_a_clean_no_op` | pass — not present, not removed, zero bytes |
| The request cannot carry a path or a session id | `core::scratch_sweep::tests::the_operator_reclaim_refuses_anything_that_is_not_a_digest` | pass — `/etc`, `../../etc`, a non-digest, a wrong-length digest, a non-hex digest, and a session id are all refused before the filesystem is touched |
| The operator path removes a linked tree without touching its target | `core::scratch_sweep::tests::the_operator_reclaim_removes_a_linked_tree_without_touching_its_target` | pass — the automatic path refuses the tree, the operator path removes it, and the outside file is byte-identical afterwards |
| A multiply-linked file no longer pins a tree | `scratch::tests::revoked_shared_cleanup_quarantines_symlinks_and_reclaims_hardlinks`; `scratch::tests::census_counts_a_multiply_linked_file_without_refusing_the_tree` | pass — the hardlinked resource is reclaimed and the outside file survives; the census counts the link and does not refuse the tree |
| The retention config has the specified defaults | `codegg_config::schema::tests::the_scratch_retention_defaults_are_the_specified_ones` | pass — 14 days, 360 minutes, 24 hours, enabled |
| A later config layer wins and an absent field inherits | `codegg_config::schema::tests::the_scratch_retention_merge_lets_a_later_layer_win_and_inherits_the_rest` | pass — `retention_days` 7 wins; the interval and grace period are inherited |
| A value below a floor is clamped, not silently accepted | `codegg_config::schema::tests::the_scratch_retention_floors_are_enforced` | pass — all three clamp to their floor |
| The operation is refused for a principal that is not the local owner | `codegg_core::authorization::tests::the_scratch_reclaim_operation_fails_closed_for_team_principals` | pass — a team principal gets `MissingScope`; the local owner is authorized |
| The operation is not reachable on the remote surface | `core::daemon_family::tests::scratch_census_answers_from_the_daemon_and_stays_local` | pass — `CoreRequest::ScratchReclaim` is `is_local_only()` |
| The operator action is audited with bounded metadata | `scratch_retention_sweep::the_operator_reclaim_disposes_of_one_named_tree_and_records_it` | pass — an `audit_event` row with action `scratch_reclaim` carries the digest, the entry and byte counts, the verdict, and the outcome, and carries no absolute path, no file name, no file content, and no session identity |
| The CLI is a dry run by default and `--apply` removes | `scratch_retention_sweep::the_operator_reclaim_disposes_of_one_named_tree_and_records_it` | pass — the dry run prints `Dry run` and removes nothing; `--apply` prints `Removed` and the tree is gone |
| A live daemon reclaims its own aged trees with no human action | `scratch_retention_sweep::the_live_daemon_reclaims_its_own_aged_trees` | pass — the aged trees are reclaimed by the daemon's own sweep, and a tree inside the grace period survives |
| The sweep removes exactly the intended trees | `scratch_retention_sweep::the_sweep_removes_the_intended_trees_and_nothing_else` | pass — the before/after manifest differs by exactly the idle tree |

## 3. Production implementation evidence

| File | Change |
|---|---|
| `src/scratch.rs` | The Unix `nlink() != 1` refusal is removed from `classify_reclaim_entry` and `ReclaimRefusal::MultiplyLinked` is deleted; `ReclaimStrictness` threads the operator relaxation through `reclaim_owned_resource_dir` → `recover_reclaim_tombstones` → `validate_and_remove_tombstone` → `validate_reclaim_tree`; `reclaim_session_scratch_dir` (renamed from `reclaim_deleted_session_scratch_dir`, because the deleted/age decision belongs to the caller), `reclaim_session_scratch_by_digest`, `reclaim_session_scratch_for_operator`, `decode_scratch_digest`, `find_session_scratch_tree`, `preview_session_scratch`; `scratch_cache_root` is the single resolver for the cache root, with a **test-only thread-local redirect** |
| `src/core/scratch_sweep.rs` | The sweep: `ScratchRetentionPolicy`, `ScratchSweepReport`, `SweepBudget`, `SweepStop`, `sweep_scratch_once`, `sweep_scratch_once_with_budget`, `reclaim_session_scratch_for_sweep`, `session_tree_is_reclaimable`, `session_owning_digest`, `session_has_accepted_turn`, `spawn_scratch_sweep`, and the operator disposition `reclaim_session_scratch_for_operator` |
| `crates/codegg-core/src/storage/immediate.rs` | The shared `begin_immediate` / `ImmediateWrite` helper, promoted out of the queue store so the sweep can use the same write-lock discipline |
| `crates/codegg-core/src/session/user_prompt_queue.rs` | Refactored onto the shared helper; the local copy of the type is removed |
| `crates/codegg-core/src/jobs/schedule_store.rs` | The import follows the moved type |
| `crates/codegg-config/src/schema.rs` | `ScratchConfig` (every field optional, so an absent field inherits), `ScratchRetentionSettings`, the three floors and the three defaults, `merge`, and `effective` |
| `crates/codegg-config/src/paths.rs` | The `scratch` merge arm |
| `crates/codegg-config/src/lib.rs` | The new re-exports |
| `crates/codegg-protocol/src/scratch.rs` | `ScratchReclaimRequestDto`, `ScratchReclaimVerdictDto`, `ScratchReclaimResultDto` |
| `crates/codegg-protocol/src/core.rs` | `CoreRequest::ScratchReclaim`, `CoreResponse::ScratchReclaim`, and `is_local_only` covering it |
| `src/core/daemon_scratch.rs` | `handle_scratch_reclaim`: the digest is resolved, the lease is decided under the writer lock, the dry run and `--apply` are honoured, and the bounded audit event is emitted |
| `src/core/daemon_family.rs` | The `Scratch` family routing for the new variant |
| `src/core/daemon.rs`, `src/core/daemon_construct.rs` | The sweep's state, its spawn on its own interval and its own handle, and the dispatch arm |
| `src/main.rs` | `codegg daemon scratch-reclaim --tree <digest> [--apply]`, the bounded fetch, and the human formatter |
| `crates/codegg-core/src/authorization/policy.rs` | The `scratch_reclaim` operation descriptor — `Opaque`, `Some(Capability::FileModify)` |
| `crates/codegg-core/src/audit.rs` | `AuditAction::ScratchReclaim` |
| `crates/codegg-core/src/audit_instrumentation.rs` | The `scratch_reclaim` coverage row and the instrumented-operation entry |
| `scripts/check_queue_write_transaction_shape.py` | Extended from one surface to three: the queue store, the shared `storage/immediate.rs`, and the new sweep module. The self-test now injects four violations |
| `codegg.example.jsonc` | The `scratch` block, annotated with the floors and the default policy |

## 4. Verification executed

Local (macOS 25.6.0 x86_64, rustc 1.89.0, load-averaged shared host,
`CARGO_BUILD_JOBS=2`):

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | pass |
| `CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets --locked -- -D warnings` | pass |
| `CARGO_BUILD_JOBS=2 cargo test -p codegg --lib scratch::` | pass — 24 passed, 0 failed |
| `CARGO_BUILD_JOBS=2 cargo test -p codegg --lib scratch_sweep` | pass — 20 passed, 0 failed |
| `CARGO_BUILD_JOBS=2 cargo test -p codegg --lib daemon_family` | pass — 3 passed, 0 failed |
| `CARGO_BUILD_JOBS=2 cargo test -p codegg --test scratch_retention_sweep` | pass — 4 passed, 0 failed |
| `CARGO_BUILD_JOBS=2 cargo test -p codegg --test scratch_usage_census` | pass — M006's census suite still passes unchanged |
| `CARGO_BUILD_JOBS=2 cargo test -p codegg-config` | pass — 113 passed, 0 failed |
| `CARGO_BUILD_JOBS=2 cargo test -p codegg-core authorization` | pass |
| `python3 scripts/check_queue_write_transaction_shape.py --self-test` | pass — four injections, each rejected |
| `python3 scripts/check_audit_coverage.py` | pass |
| `python3 scripts/check_config_merge_coverage.py --self-test` | pass |
| `scripts/verify.sh quick` | pass — every guard green, and `cargo check --workspace --all-targets --locked` clean |

Hosted CI on the pull-request head: recorded in this section after the run
completes.

### 4.1 The WP0 census (M006's census, run against a populated scratch tree)

The census was run against a **real daemon** over the local socket, against a
population planted through the daemon's own provisioning path — a real tree,
with a real owner record, named by a real digest.

Environment: an isolated `HOME`, `CODEGG_DAEMON_HOME`, and `CODEGG_DATA_HOME`
under a canonical temporary root; the real `codegg` binary; the daemon's own
retention sweep **disabled** for the measurement, because the planted trees
are aged past the orphan grace period and own no durable session, so a live
sweep would reclaim them as orphans before the census could measure them.

The measured population:

| Tree | Age | Bytes | Verdict |
|---|---|---|---|
| `scratch-census-day` | 1 day | 4,187 | reclaimable |
| `scratch-census-week` | 7 days | 28,763 | reclaimable |
| `scratch-census-month` | 30 days | 122,971 | reclaimable |
| `scratch-census-year` | 365 days | 1,495,131 | reclaimable |
| `scratch-census-linked` | 3 days | 91 | **refused** — `symbolic_link` |
| `scratch-census-hardlinked` | 5 days | 105 | reclaimable, `multiply_linked_count` 1 |

Six trees, 1,651,248 bytes, not truncated. Every tree reports
`owner_record_present: true`.

**The decision this produced.** The census does **not** contradict §6.1's
defaults, so the defaults were not adjusted. It does show exactly what the
policy is for: the oldest tree dominates the byte total by two orders of
magnitude, so a policy that bounds *age* is what bounds the *bytes*; there is
no byte threshold that could be derived from this population that would not
be an artefact of one planted file.

**What the census did not establish.** The population is **synthetic**: no
live model turn was driven, so the byte distribution reflects planted files
rather than real agent output. The default therefore remains **specified**,
not **measured**. It is now **consulted** — the plan's WP0 ran, and the census
agreed with the shape of the policy — but the number 14 is still derived from
the disposal contract, and this closure does not claim otherwise.

The multiply-linked tree is the one place where the census and WP1 meet: it
reports `multiply_linked_count: 1` and `reclaimable: true`. Before WP1, that
tree would have been refused and pinned forever. The census measured the
parity fix.

### 4.2 The before/after manifest across a sweep

`scratch_retention_sweep::the_sweep_removes_the_intended_trees_and_nothing_else`
plants three trees — one idle, one fresh, one leased — records the sessions
root's manifest, runs one sweep pass, and records it again. The two manifests
differ by exactly the idle tree:

```
before                      after
5afe93ef5c7174304b318…      5afe93ef5c7174304b318…
5fcea30922565e27ce1b77…      (removed — the idle session's tree)
8cc5b84d63b01873878db…      8cc5b84d63b01873878db…
```

The leased and fresh trees are byte-identical before and after, and the pass
reported `reclaimed_idle` 1, `skipped_leased` 1, `skipped_active` 1, `failed`
0.

### 4.3 The live-daemon observation

`scratch_retention_sweep::the_live_daemon_reclaims_its_own_aged_trees` plants
three aged trees and one fresh tree, starts a **real daemon**, and waits. The
daemon's own sweep reclaims the three aged trees with no human action, and
the fresh tree survives.

## 5. Invariant review

- **The disposable-cache contract is preserved verbatim.** "Cache contents are
  disposable and may be removed by host cache maintenance" is unchanged in
  `architecture/process-tool-execution-ownership.md` and in the skill.
- **No size traversal on the agent hot path.** The sweep's decision reads a
  durable timestamp and three durable lease facts. It never walks a tree, and
  it runs on its own interval.
- **No global scan per tool call.** The sweep is never invoked from the tool
  path at all. The census is not either.
- **The reclaim path's serialization is reused, not duplicated.** The sweep
  calls the existing owner-validated tombstone path; it adds no second lock
  for a tree the reclaim path already serializes.
- **The digest computation is single-sourced.** `session_scratch_dir`,
  `existing_session_scratch_dir`, and the sweep's owned-digest map all call
  `session_scratch_digest`.
- **The entry rule is single-sourced.** `validate_reclaim_tree` and the
  census both call `classify_reclaim_entry`, so a report cannot claim a tree
  is reclaimable when the validator would refuse it.
- **The existing reclamation tests still pass.** The tests in `src/scratch.rs`
  and `src/core/daemon_sessions.rs` pass unchanged, apart from the deliberate
  Unix hardlink expectation in WP1.

## 6. Failure and recovery review

- **A pass that hits a bound records which bound stopped it.** A truncated
  pass is never presented as a complete sweep. The three bounds are the number
  of candidates examined, the number of trees reclaimed, and a wall-clock
  budget.
- **A tree that vanishes mid-sweep is recorded, not failed.** A concurrent
  reclaim removed it; the pass continues and counts it under `vanished`.
- **A refusal is not a failure.** A tree the validator refuses is counted
  under `refused`, not `failed`, because a healthy sweep will keep seeing it
  and reporting it as a refusal would report a healthy sweep as broken.
- **A restored session is not lost.** The sweep read the session set before
  the restore; the re-read inside the writer lock is what sees it, and a
  restored, active session keeps its tree.
- **A daemon restart clears nothing that matters.** The sweep is stateless: it
  reads the durable session set and the filesystem, and the first pass after a
  restart rebuilds everything.
- **Contention with a turn admission.** The lease is decided inside the writer
  lock and the reclaim happens inside it, so a turn cannot be admitted between
  the decision and the removal.
- **A failed sweep does not damage anything.** The reclaim itself is the
  owner-validated tombstone path, so a failure leaves the tree quarantined
  under a tombstone, where the census can see it and the operator can dispose
  of it.

## 7. Migration and compatibility review

- **No storage change.** No migration, no `STORAGE_LAYOUT_VERSION` bump, no
  schema change. The lease reads tables that already exist.
- **The configuration change is additive.** A new `scratch` section with four
  optional keys. Every value is clamped upward at its floor rather than
  rejected, so a mistyped value cannot become "reclaim every tree now" and
  cannot stop the daemon from starting.
- **The protocol change is additive.** `PROTOCOL_VERSION` stays `2`: the
  request is a new variant of an existing enum, and a client only sends it
  deliberately. A client talking to a daemon that cannot answer gets the
  bounded error M006 introduced, not a hang.
- **The policy is read once, at construction.** A change to `scratch.*` needs
  a daemon restart. That is recorded in `architecture/config.md`.
- **The scratch layout is unchanged**, so no existing scratch tree is
  invalidated.

## 8. Security review

- **The sweep removes only what the daemon owns.** Every reclaim goes through
  the existing owner-validated tombstone path: the owner record must exist and
  match the digest, and the tree must be inside the daemon's own sessions
  root. The sweep adds no path-shaped input at all.
- **The operator path relaxes the entry-content refusals only.** Ownership,
  containment, and the owner-record check are unchanged, so it can only ever
  remove a tree the daemon itself created and still owns. `remove_dir_all`
  does not follow a symbolic link, so the link's target is untouched — proved
  by a test that compares the outside file's bytes before and after.
- **The request cannot name a path or a session.** The wire request carries one
  digest-shaped string, which is validated as a digest before it reaches the
  filesystem. The durable-to-disk mapping is one-way, so a session id cannot be
  recovered from a digest and cannot be named in a request.
- **The operation is scoped `Opaque` with `FileModify`.** A tree has no
  project linkage, so a team principal has no grant to resolve it against and
  the request fails closed; only the local-owner broad policy authorizes.
- **The operation is local-only.** `CoreRequest::is_local_only` gates the
  remote WebSocket transport, so a remote client never reaches it.
- **The operation is audited.** Every attempt is recorded as
  `scratch_reclaim` with bounded metadata — the digest, the entry and byte
  counts, the verdict, the refusal reason, and the outcome. The metadata
  carries no absolute path, no file name, no file content, and no session
  identity; a test asserts each of those absences against the real audit row.
- **The sweep grants no access and widens no authority.** It removes the
  daemon's own disposable cache and nothing else.

## 9. Documentation and operations

- `architecture/security.md` — the sweep paragraph and the operator-disposition
  paragraph, including the soft-deleted rule, the orphan rule, the three
  bounds, and the statement that the operator path relaxes the entry-content
  refusals only.
- `architecture/config.md` — the `scratch` key in the key-config-sections
  list, with the defaults, the floors, and the restart requirement.
- `architecture/process-tool-execution-ownership.md` — the disposable-cache
  wording is preserved; the sweep and the operator disposition are named.
- `docs/daemon.md` — `codegg daemon scratch-reclaim` and the sweep's
  configuration.
- `docs/tools.md` — the scratch paragraph now states that the tree is swept
  and measured, not merely measurable.
- `architecture/authorization.md` — the `scratch_reclaim` row.
- `architecture/audit.md` — the audit-action taxonomy, with a dated
  correction note.
- `.opencode/skills/tool-execution/SKILL.md` — the cleanup paragraph now
  names the sweep, the lease, and the operator disposition, so the skill and
  `architecture/security.md` agree.
- `codegg.example.jsonc` — the annotated `scratch` block.
- `architecture/overview.md` — the `## Verified counts` table is remeasured:
  `CoreRequest` variants **261** (237 struct + 24 unit), `CoreResponse`
  variants **171**, and authorized operations **255**.

## 10. Unresolved findings

None that block the milestone. Three facts are recorded so that a successor
does not have to rediscover them:

- **The retention default is *specified*, and now *consulted*.** The census ran
  (WP0) and did not contradict the default, so the default was not adjusted.
  The measured population was synthetic, so the number itself is still
  contract-derived. A future milestone that drives real turns could measure the
  real age distribution and revisit the number.
- **A tree the automatic path still refuses stays quarantined until an
  operator disposes of it.** That is the residual this milestone names
  explicitly rather than leaving silent. The operator action is audited, so
  the disposition is a record, not an erasure.
- **The Windows Required native sandbox containment backend does not exist.**
  That is the C002 blocker, tracked in the separate native-qualification
  corrective addendum and deferred to its own workstream. This milestone
  claims no Windows containment. What it does claim for Windows is the
  reclaim-validator **parity** fix: the multiply-linked refusal was
  Unix-only, and removing it makes the two platforms agree.

## 11. Roadmap disposition

`plans/subsystems/managed-agent-scratch-roadmap.md` §7's M007 section moves
from `ready` to `closed` and names this record; §12's M007 row carries the
same status and the closure path. §6's milestone graph is reconciled: M006's
entry, which was still labelled `ready` after M006's own closure, now reads
`closed`, and M007's reads `closed` with this record.

## 12. Registry updates

`plans/registry.md`: the subsystem row now reads `M001/M002/M004/M005/M006/M007
closed; M003 conditional`; the M007 row moves to **closed** and names this
record; the M007 row's M006 dependency text is reconciled with the new state,
including the WP0 census result.

## 13. Factual corrections

- **`plans/subsystems/managed-agent-scratch-roadmap.md` §6** carried a stale
  label: M006's graph entry still read `[ready]` after M006's own closure
  moved the milestone to `closed` in §7 and §12. The label is corrected here.
  The correction is to the roadmap, not to M006's closure record, which is
  immutable.

## 14. Handoff notes

- **The scratch tree's growth is now bounded, and the bound is the daemon's
  own.** The sweep reclaims a session's tree when the session is idle past the
  retention window, a soft-deleted session's tree once the lease is clear, and
  a tree no durable session owns once it has been untouched past the orphan
  grace period.
- **The lease is durable state, not a new primitive.** A successor that needs
  a liveness signal should read `turn_admission_receipt`, `agent_task`, and
  `job` before inventing a table.
- **The operator disposition is the extension point for the residual.** It
  relaxes the entry-content refusals only. A successor that wants to reclaim
  more kinds of tree should change `classify_reclaim_entry` — and then the
  census's verdict changes with it, and the operator path inherits it.
- **The census is the measurement, and it still reclaims nothing.** It is the
  instrument for the next retention question: the real age and byte
  distribution of a live install, measured against real agent output rather
  than a planted population.
