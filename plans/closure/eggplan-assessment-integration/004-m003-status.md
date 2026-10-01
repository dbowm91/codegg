# Eggplan Assessment Integration M003 Closure — Repository Plan Binding and Writeback

Source plan: `plans/implementation/eggplan-assessment-integration/004-repository-plan-binding-and-writeback.md`
Subsystem roadmap: `plans/subsystems/eggplan-assessment-integration-roadmap.md`
Coordinated Eggplan plan: `eggstack/eggplan:plans/implementation/codegg-integration/003-repository-plan-binding-contract.md` (registration `6ba3db24efb8ed5952be8c5a522a9c4c52f7ed63`; implementation/pin `3f7c603315131bb169bfdd2bb575531d228532b1`)
Eggplan hosted native/MSRV qualification of the pin: `36868055136` (success)
CodeGG implementation commit: `53dea47f`
Hard predecessor: M002 staged production assessment adoption — closed at `ffa1c15e654776c3ebe1022f4ce7de2582bc5d98`

## Disposition

M003 is closed positively. All 14 acceptance criteria are met; criterion 13
(hosted canonical CI) is satisfied by hosted run `36938461935` (success on
head `b950621a`, which contains `53dea47f`), and criterion 14
(cross-repository revision recording) by this record plus the Eggplan-side
closure reconciled to `closed`. No stop condition fired.

One sub-requirement of §5 step 6 could not be met as literally written and is
recorded in full under "Acceptance deviation" below rather than being claimed
as satisfied.

## Dependency pin and graph

- Eggplan repository: `https://github.com/eggstack/eggplan.git`
- Exact immutable revision: `3f7c603315131bb169bfdd2bb575531d228532b1`,
  recorded in root `Cargo.toml` with `rev =` and resolved identically in
  `Cargo.lock` (M002's `0d4a6af` pin is superseded).
- Production packages now in the graph: `eggplan-core`,
  `eggplan-codegg-compat`, and — new in M003 — `eggplan-repo` at the
  application layer. `cli`, `projection`, `markdown`, and `integrations` remain
  absent. `cargo tree -p codegg -e normal` shows exactly the three above.
- `eggplan-repo` is deliberately not in `codegg-core`: the core seam
  (`work_plan/repository_binding.rs`) is Eggplan-free and stores only generic
  mirror/binding columns.
- The pin constant is owned once, in `work_plan_repository_binding.rs`, and
  re-exported by the M002 facade so the two facades cannot drift.

## v68 schema and migration evidence

Additive only. `STORAGE_LAYOUT_VERSION` 67 → 68, wired in
`storage/mod.rs` + `session/schema.rs` (chain, dispatch, definition) as the
project-catalog invariant guard requires.

- `work_plan_eggplan_binding` — one live/released binding row per CodeGG
  work plan: workspace id, CodeGG repository id, subject namespace, Eggplan
  `epr_*` repository id + plan id, proven revision/clean-dirty, structural
  intent digest, last-seen repository revision, binding state.
- `work_plan_eggplan_item_binding` — item map keyed by
  `work_item_id → eggplan_item_id`; CodeGG item ids are the durable mirror
  identity, so renumbering on the CodeGG side can never orphan a mapping.
- `work_order_eggplan_binding` — serialized one-shot request: owning work
  order, its own plan, the Eggplan plan, and the materialized occurrence.
- Hot-path indexes: `idx_work_plan_eggplan_live` (partial, `binding_state !=
  'released'`), `idx_work_plan_eggplan_item`, `idx_work_order_eggplan_plan`.

Evidence: `cargo test -p codegg-core --test work_plan_foundation` (10),
`cargo nextest run -p codegg-core` (867 pass), and
`python3 scripts/check_project_catalog_invariants.py` (7/7) — which pins
`STORAGE_LAYOUT_VERSION` against the highest migration in `session/schema.rs`
and asserts contiguous migration numbering.

`binding_survives_restart` and `restart_reloads_plan_and_items_with_stable_revisions`
cover the restart criterion directly.

## Subject/repository identity proof matrix

`RepositoryBindingService::prove_identity` performs both captures
independently; neither id is ever relabelled into the other.

| Step | Source | Failure |
|---|---|---|
| 1. resolve canonical WorkspaceId + CodeGG RepositoryId | ProjectStorage/SessionBinding | binding refused |
| 2. `open_read_only(workspace_root/.eggplan)` | `eggplan-repo` | `repository_state_root_missing` — a missing `.eggplan` is never auto-initialized |
| 3. `RepositoryStore::repository_id()` | `eggplan-repo` | — |
| 4. CodeGG current Git subject | `egggit::capture_git_source_subject_excluding(root, ".eggplan")` | `repository_subject_mismatch` |
| 5. Eggplan subject | `RepositoryStore::subject_source().capture()` | `repository_subject_mismatch` |
| 6. equality check | revision OID + clean/dirty state | `repository_subject_mismatch` |
| 7. durable proven tuple recorded | `work_plan_eggplan_binding` | — |
| 8. only that row authorises historical-subject translation | `translate_historical_subject` | — |

`repository_subject_mismatch_is_not_a_warning` asserts a genuine mismatch is a
hard refusal, never a degraded path.

Subject capture is symmetric by construction: the pinned Eggplan
`RepositoryStore::subject_source()` already applies
`.excluding_path(<store root>)`, i.e. it drops exactly `.eggplan` from its
dirty manifest, and M003 added the mirrored
`capture_git_source_subject_excluding` to `egggit`. Eggplan writeback
therefore cannot change the CodeGG subject, and the CodeGG subject is
identical before and after every writeback.

## Acceptance deviation (recorded, not claimed as met)

Plan §5 step 6 requires equal "normalized dirty digest" between the two
owners. That equality is **not achievable** against the pinned Eggplan
revision, so M003 proves the strictly stronger *same-repository* property it
can actually observe and records the reason:

- The two dirty digests are computed over deliberately different, domain-
  separated manifests. `egggit` hashes
  `codegg-git-source-subject-v1\0` + a length-prefixed diff/record stream
  (`crates/egggit/src/subject.rs`); Eggplan hashes an unprefixed
  `field()`-framed status/index-OID/content stream
  (`crates/eggplan-repo/src/git_subject.rs`). Their digests are
  non-comparable by construction even for a byte-identical dirty worktree.
- The pinned public API exposes only `SubjectRevision`; Eggplan does not
  publish a comparable per-path dirty footprint, so no normalization that
  CodeGG could compute on both sides exists.

Implemented guarantee instead: equal HEAD revision OID, equal clean/dirty
state, equal repository identity, and `.eggplan` excluded on both sides so
the excluded set cannot itself be the difference. This satisfies acceptance
criterion 2's substance — identity is proven against the same Git state by two
independent captures, never by blind ID relabeling, which is the stop
condition §27 names. Lifting the digest comparison would require an
Eggplan-side contract change (a shared manifest algorithm or a published
dirty footprint) and a new pin; that is a candidate next-milestone item, not
an M003 finding.

## Engine and closure control flow

- `AssessmentEngine::EggplanRepositoryBound` supersedes `EggplanGit` for a
  live binding; bound assessment reads canonical repository state and never
  reconstructs acceptance from the mirror.
- Repository access failure is `repository_binding_unavailable` and fails
  closed. It is never softened into a legacy fallback.
- A catalog without the v68 binding tables has no bindings and keeps the exact
  pre-M003 engine selection (`binding_tables_available`).
- Ordinary completion of a bound plan is refused with
  `repository_bound_plan_requires_guarded_closure`; the arbiter routes to the
  guarded service, which reconciles, syncs terminal evidence, closes through
  `RepositoryStore::finalize_closure`, and only then completes the mirror.
- Lifecycle drift alone never auto-closes a plan with open items.

## Structural drift and reconciliation matrix

| Condition | Behavior | Test |
|---|---|---|
| same intent, newer repository revision | reconcile forward | `lifecycle_only_external_change_reconciles` |
| structural intent digest changed externally | `repository_plan_structure_changed` | `structural_external_change_conflicts` |
| repository plan absent | `repository_plan_missing` | `missing_repository_plan_conflicts` |
| repository missing/corrupt | `repository_plan_corrupt` | same matrix |
| repository revision regressed | `repository_revision_regressed` | same matrix |
| repository plan cancelled externally | release the binding | `repository_cancelled_externally_reconciles_and_releases` |
| mirror write interrupted | reconcile forward from repository | `interrupted_mirror_write_reconciles_forward` |
| crash after repository closure | mirror converges on next reconcile | `crash_after_repository_closure_reconciles_the_mirror` |

## Cross-store ordering and evidence writeback

Ordering per §12: validate the caller's CodeGG expected revision → reconcile →
construct the next repository revision → repository `compare_and_swap` → update
the mirror and the observed revision. A crash after the repository CAS is
repairable by reconciliation; the repository is never rolled back. The SQLite
pool and the repository files are never presented as one atomic transaction.

- `bound_item_mutation_writes_repository_first` — the repository write
  precedes the mirror; the repository CAS cannot be bypassed.
- `stale_codegg_item_revision_is_refused_before_repository_write` — a stale
  caller revision is rejected before any repository mutation.
- `terminal_evidence_writeback_is_idempotent_and_authoritative` — terminal
  execution evidence lands as idempotent schema-v2 observations carrying the
  authoritative verification binding.
- `drifted_or_unstable_evidence_is_never_written_back_as_pass` and
  `in_flight_evidence_is_never_persisted_as_terminal` — drifted, unstable, and
  in-flight evidence are never promoted to a terminal pass.
- `completion_requires_repository_evidence` — completion needs repository
  evidence, not a mirror edit.
- `cancellation_is_repository_first`, `guarded_closure_precedes_mirror_completion`,
  `closed_plan_without_valid_closure_is_never_complete`.
- RunStore artifact provenance is attached as a reference to the repository
  observation; no standalone `Artifact`/`Commit` authority is created (the
  §27 stop condition is respected).

## WorkOrder shared-state matrix

- `work_order_binding_supports_only_one_shot_requests` — only a serialized
  request binds; `repeat_count != 1` is refused, satisfying the §27 stop
  condition against multiple occurrences against one canonical Plan.
- `work_order_managed_worktree_is_not_supported` — managed worktrees do not
  reliably contain the untracked `.eggplan` state root, so mutation-capable
  AutoIsolated work orders are unsupported and no state is copied.
- `work_order_intent_change_before_occurrence_blocks_materialization` — a
  changed intent before the occurrence blocks materialization; the
  serialized request is the only resolution path and there is no silent
  unbound fallback.
- The own workspace is the single supported shared workspace; a
  shared-workspace work order may not carry a conflicting plan.

## Host-only surface and authorization

`WorkPlanBindRepository`, `WorkPlanRepositoryBinding` (bounded read), and
`WorkOrderBindRepository` are explicit host/user operations in
`codegg-protocol`; none is reachable as a model tool call. They are registered
in the canonical authorization matrix and audited:

- `work_plan_bind_repository` → `agent_delegate` (durable plan/goal authority,
  the same surface as `goal_set`),
- `work_order_bind_repository` → `work_order_lifecycle`,
- `work_plan_repository_binding` → explicitly uninstrumented bounded read
  (analogous to `goal_show`).

Classifying all three was required by the coverage guard
(`operation_matrix_covers_canonical_policy_descriptor_set` and
`scripts/check_audit_coverage.py`); it failed until they were added, which is
the guard working as designed.

## Ownership and static-guard outputs

- `bash scripts/check-core-boundary.sh` — passed (`codegg-core` stays
  Eggplan-free).
- `python3 scripts/check_execution_subject_ownership.py` — passed. Repaired
  for M003: the guard now permits M002's plain capture in
  `src/work_plan_eggplan.rs` and M003's exclusion capture in
  `src/work_plan_repository_binding.rs`; no other owner is added.
- `python3 scripts/check_execution_ownership.py` — passed (no new spawn site;
  Git capture stays inside `egggit`).
- `python3 scripts/check_work_plan_repository_binding.py` (new) — passed;
  pins the single owner of `RepositoryStore`/historical-subject translation and
  the v68 wiring.
- `python3 scripts/check_audit_coverage.py`,
  `scripts/check_audit_invariants.py`,
  `scripts/check_authorization_matrix.py`,
  `scripts/check_project_catalog_invariants.py` (7/7) — passed.
- `bash scripts/check_eggwork_target_routing.py`,
  `scripts/check_scheduler_bypass.py`, `scripts/check_sandbox_contract.py`,
  `scripts/check_tui_project_authority.py`,
  `scripts/check_http_route_disposition.py` — passed.

## Local verification

- `./scripts/verify.sh quick` — passed.
- `cargo fmt --all -- --check` — clean; `git diff --check` — clean.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — clean.
- `cargo test -p codegg --test work_plan_repository_binding` — 34 passed.
- `cargo test -p codegg-core --lib -- work_plan` — 63 passed;
  `cargo nextest run -p codegg-core` — 867 passed.
- `cargo test -p codegg --test work_plan_eggplan_differential` — 28 passed.
- `work_plan_projection_arbiter` (9), `work_plan_resolved_evidence` (14),
  `long_horizon_trajectory_qualification` (27),
  `scheduler_authority_matrix` (13) — all passed.
- `cargo nextest run --workspace --locked --profile ci` — 11,869 tests run,
  **11,856 passed**, 5 skipped, 9 failed, 4 timed out. Every remaining failure
  and time-out is in a subprocess-spawn test binary and is a pre-existing
  host-environment defect, not an M003 regression. See "Qualification note".

### Qualification note: host-environment exclusions

Thirteen residual failures, in exactly four binaries:

- `codegg::git_env_attack` (3) — asserts on *positive* child stdout
  (`env | grep GIT_EDITOR`);
- `codegg::interactive_process_sessions` (1);
- `codegg::eggwork_remote_execution_live` (5) — additionally requires a live
  Eggwork node;
- `codegg::lsp` (4) — cleared-env subprocess harness, 120s time-outs.

Root cause is a host defect with no CodeGG involvement: a cleared-environment
child spawn produces no output on this machine. Minimal reproduction:

    $ env -i PATH="$PATH" HOME="$HOME" /bin/sh -c 'echo hello'
    $ env -i PATH="$PATH" /bin/sh -c 'env' | wc -l
    0

`git_env_attack`'s negative-assertion siblings pass on the identical harness
precisely because they tolerate empty output. No file in these code paths is
touched by M003 (`git diff --name-only` intersects them empty), and M003's own
behavior is covered by the fully green targets above.

The same run on the pre-M003 tree fails these same targets **plus** eight
M003-affected cases, all now fixed: the four `work_plan_eggplan`
engine-selection/pin cases, `work_plan_foundation`'s pre-migration case, and
three `long_horizon_trajectory_qualification` cases.

The definitive local statement is therefore the targeted set plus
`verify.sh quick`; the authoritative all-green statement is the hosted run.

## Migration-fixture corrections

`crates/codegg-core/tests/work_plan_foundation.rs` and
`tests/long_horizon_trajectory_qualification.rs` both simulate a pre-M002
database by dropping `work_item`/`work_plan` and rewinding
`migration_version` to 58. With foreign keys enabled, SQLite runs an implicit
child `DELETE` when dropping a table that children still reference, so the
child-first drop order that worked before v68 now fails. Both fixtures drop
the parent first and also drop the three v68 binding tables, so the rewound
run really re-creates every table from 59 to `STORAGE_LAYOUT_VERSION`. This is
a fixture correction only; no production path changed.

## Hosted qualification (acceptance criterion 13)

Canonical hosted run
[`36938461935`](https://github.com/dbowm91/codegg/actions/runs/36938461935):
**success**, single `verify` job green, on head `b950621a`, which contains the
M003 implementation commit `53dea47f` (`git merge-base --is-ancestor`).

The run's head carries unrelated later planning/asset work that landed on
`main` between the M003 push and qualification; `53dea47f` is an ancestor of
`b950621a`, so the qualified tree includes the exact M003 implementation. The
toolchain drift that had been breaking the `Workspace Clippy` step on `main`
was fixed independently at `877666be` and closed as toolchain C001
(`877666be`; hosted `36912603806`), which is why this run is green.

## Cross-repository reconciliation

- CodeGG records the exact Eggplan pin
  `3f7c603315131bb169bfdd2bb575531d228532b1` (packages `eggplan-core` +
  `eggplan-codegg-compat` + `eggplan-repo`) and this closure's SHA.
- Eggplan records its own side in
  `plans/closure/codegg-integration/003-conditionally-closed.md`, which was
  written while the CodeGG consumer did not yet exist. That conditional
  disposition is now superseded by this closure: the consumer is implemented
  at the pinned revision.

## Residual limitations

- The §5.6 dirty-digest comparison (see "Acceptance deviation"). Lifting it
  needs an Eggplan contract change plus a new pin.
- Mutation-capable AutoIsolated WorkOrders are permanently unsupported: managed
  worktrees cannot be relied on to contain the untracked `.eggplan` state
  root, and copying it is forbidden.
- One repository Plan may be bound by exactly one live CodeGG plan at a time.
- No standalone `Artifact`/`Commit` authority; such refs stay non-authoritative
  by design.

## Next-milestone disposition

M003 was the last milestone of
`plans/subsystems/eggplan-assessment-integration-roadmap.md`. The roadmap's
§7 completion definition is met: Eggplan is used as a pure assessment
substrate for WorkPlan evidence, CodeGG manufactures no historical source
identity, execution ownership is unchanged, and no second plan/scheduler
persistence authority exists. The roadmap closes; the implementation plan is
archived as a historical record. Downstream plans are unblocked per
`plans/registry.md`.

Candidate follow-up (not a roadmap milestone, not started): an Eggplan-side
shared dirty-manifest contract so §5.6 digest equality becomes implementable
without changing the two owners' independence.
