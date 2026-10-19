# Linux Trusted Git Cost Corrective C007 — Closure Status

Status: closed — the production publisher, the durable journal, the crash reconciler, and the contained staging step under M004/M005 are all proven; the obtained network fact remains unreported and is owned by C014

Source implementation plan:

- `plans/implementation/linux-trusted-git-cost-corrective/007-production-private-index-publication-vertical.md`

Source subsystem roadmap:

- `plans/subsystems/linux-trusted-git-cost-corrective-roadmap.md#milestones-and-handoffs`

Repository baseline reviewed: `6efd5ebdf90f91fc24dcac7f2827f4ca9e355135`

## 1. Executive finding

C007's production vertical exists: an authorized `git add` is staged into a
private index in daemon-owned scratch, the intent is recorded in a durable
journal, and the index is published under Git's own `index.lock` with a
compare-and-swap against the index generation the staging observed. A
daemon-start reconciler resolves any publication a prior generation left
in flight, and the `Add` mutation is reachable from an ordinary dispatch
caller (`git` tool `stage_paths`, `commit` tool).

The **contained** staging step is now **proven on a qualified host**. The
hosted Linux runner executed
`production_private_index_publisher_stages_and_publishes_under_containment`
against this branch's exact source and it passed, together with the whole
M004/M005 suite — first on run `39318206124`, and then again on run
`39339024314` at the branch's final commit `78359fd3`, which is the run that
qualifies the delivered source (§4).

One thing remains **not** established, and it is not waived:

- The **obtained network fact** is still not reported anywhere in the system.
  C014's open item stands. This closure makes no M005 production claim, and
  the receipt says so by construction: the receipt states the published
  resource, and it states the network boundary explicitly as `not_reported`,
  because the system cannot report what it obtained.
The publication half — the journal, the compare-and-swap, the lock, the
object import, the atomic replacement, and the recovery — is proven on this
workstation, because the staging step is behind a trait and the proof
exercises the production code above that boundary.

ADR-0044 remains **proposed** and is **not** accepted. C005 remains
**blocked**. C010 no-admission and M003 blocked status are unchanged. No
Linux host is admitted.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Durable journal with a typed state machine, every state produced | `crates/codegg-core/src/git_publication.rs`; migration v84; `journal_records_reads_and_lists_an_unresolved_intent`, `journal_refuses_a_duplicate_intent`, `journal_lists_only_unresolved_intents` | pass | `STORAGE_LAYOUT_VERSION` 83 → 84. Five states; `committed`, `aborted`, `ambiguous` are terminal and never transition. |
| Compare-and-swap on the real index | `GitIndexPublisher::publish_under_lock`; `a_concurrent_external_writer_produces_a_typed_conflict_and_is_not_overwritten` | pass | The external writer's index is not overwritten and the publication is not applied. |
| Git-compatible locking | `publish_under_lock` (same-filesystem hard link onto `index.lock`); `a_held_index_lock_produces_a_typed_conflict` | pass | The lock is correct from the instant it exists, so a leftover lock is attributable. The other writer's lock is untouched. |
| Atomic replacement within the gitdir | `publish_under_lock` (rename into the gitdir, then fsync) | pass | Same filesystem by construction: the temporary file is written in the gitdir. |
| A failed staging leaves the repository byte-identical | `a_failed_staging_leaves_the_repository_byte_identical` | pass | The real index bytes and the real object database are unchanged, and nothing is recorded in the journal. |
| Crash recovery: committed / aborted / ambiguous | `reconciler_closes_a_committed_publication`, `reconciler_aborts_an_unchanged_index_and_discards_the_private_artifact`, `reconciler_escalates_an_ambiguous_publication_and_retains_the_private_artifact` | pass | Ambiguity is never reported as success; the private index is retained for inspection. |
| Recovery is idempotent across restarts | `reconciliation_is_idempotent_across_restarts` | pass | A second pass resolves nothing and changes nothing. |
| A leftover lock is removed only when attributable | `reconciler_removes_only_a_leftover_lock_it_can_attribute` | pass | A lock that holds another writer's bytes is left alone. |
| An ordinary dispatch caller reaches the publisher | `GitMutationExecutor::execute` `Add` arm; `git` tool `stage_paths`; `commit` tool; the **bash route** (`BashTool::git_mutation_executor`); `the_bash_git_executor_carries_the_publication_backend` | pass | The `Add` arm routes to the publisher, and every caller that can reach an authorized `Add` — including a bash-routed `git add` — is given the backend. |
| An unconfigured backend refuses rather than downgrades | `an_unconfigured_backend_refuses_the_add_mutation` | pass | No silent fallback to the in-place `git add` route. |
| A path outside the repository is refused before any child runs | `a_path_outside_the_repository_is_refused_before_any_child_runs` | pass | Nothing is recorded in the journal. |
| The publisher executes no hook, filter, helper or worktree-controlled code | `PRIVATE_GITDIR_CONFIG`; the private gitdir is authored by the daemon and contains no `filter.*` section; `core.attributesFile = /dev/null`; the staging child's configuration is pinned by the daemon (`GIT_CONFIG_NOSYSTEM=1`, `GIT_CONFIG_GLOBAL=/dev/null`, `GIT_ATTR_NOSYSTEM=1`); the hosted probe `production_private_index_publisher_stages_and_publishes_under_containment` | pass | The staging child's configuration is pinned by the daemon, so a repository-controlled clean/smudge filter has no command to run. The hosted probe is **empirical**, not a design claim: it writes `tracked.txt filter=probe` into the source repository's `.gitattributes` and configures `filter.probe.clean` in the **source repository's own config** to a script that would create a `source-filter-ran` marker, then asserts the marker does not exist. The worktree `.gitattributes` *is* read by the child — that is why the probe is meaningful; what the child cannot obtain is a **command** to run for it. |
| The contained staging step under M004/M005 | `ContainedGitIndexStaging`; `production_private_index_publisher_stages_and_publishes_under_containment` in `tests/linux_m004_process_ipc.rs`; hosted `process-ipc` job | pass | Hosted run `39339024314` (2026-10-19, commit `78359fd3`, the branch's final source): the isolated C007 step reports `running 1 test` / `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 16 filtered out`, and the same test is among the 17 that passed in the full M004/M005 suite. The whole production path ran under containment on a qualified host. The first hosted attempt (`39313802793`) was cancelled by the job's time budget and produced no evidence either way; the budget defect was corrected rather than the evidence inferred (§4). |
| The receipt states the obtained network boundary truthfully | `GitIndexPublicationReceipt::network_boundary`; `ObtainedNetworkBoundary::NotReported` | pass | The receipt states the boundary **explicitly** as `not_reported` rather than staying silent about the network, so a caller never has to infer an omission. The type carries one variant because one answer has a producer: the system still cannot report the fact it obtained (C014's open item), and the reported case will be added with its producer, not as a claim. |
| A non-`Add` mutation is never published | `a_non_add_mutation_is_never_published`; the `Add` arm of `GitMutationExecutor::execute` | pass | A non-`Add` mutation keeps the legacy route, the branch is created, and the publication journal records **no** intent — the publisher never sees it, so a caller cannot route around the publisher's preflight by choosing a different mutation. |
| The `Add` arm runs the **same** preflight every other mutation runs | `execute_add_publication`'s `env_policy.apply` gate in `src/git_mutations.rs`; `stage_paths_returns_paths_in_delta`, `stage_all_marks_changes_staged`, `git_stage_and_branch_mutation_emit_once_each` | pass | The `Add` arm renders the operation and applies the environment policy **before** the publisher is consulted, so a request the policy refuses is refused with the policy's typed outcome, not with a publication error. The workspace sweep found the arm short-circuiting this gate; see §4. |

## 3. Production implementation evidence

- `crates/codegg-core/src/git_publication.rs` — the durable journal: the
  five-state machine, the fingerprint type, and the store. The store's
  `mark` is a guarded read-then-write inside a `BEGIN IMMEDIATE`
  transaction, so two daemons racing to publish the same repository cannot
  both believe they won.
- `crates/codegg-core/src/session/schema.rs` — migration v84, additive: one
  table and one index. `STORAGE_LAYOUT_VERSION` is 84, and the
  project-catalog invariant guard compares the two live. The v84 `CREATE
  TABLE` / `CREATE INDEX` statements live **inline in `schema.rs`**, like every
  other table, rather than in a statement list exported by the core module —
  `architecture/overview.md`'s "Database tables" row lists `schema.rs` as its
  `Source`, so the DDL has to live there for that count to stay measurable.
- `src/git_publication/` — the production vertical: `publisher.rs` (private
  staging, compare-and-swap, lock, object import, atomic replacement),
  `staging.rs` (the contained staging step and the `GitIndexStaging`
  boundary), and `reconciler.rs` (the startup pass).
- `src/git_mutations.rs` — the `Add` arm. `GitOperation::Add` routes to the
  publisher; every other operation keeps the legacy route. `Add` remains a
  workspace-scoped, non-destructive mutation: the Git execution policy
  already classifies it as a contained operation whose only capability
  requirement is `CleanSmudgeFilter`, and it does **not** require
  `DestructiveRepositoryChange`.
- `src/core/daemon_bootstrap.rs` — the reconciler runs as bootstrap phase 5
  (after `recover_jobs`), in the in-process path and in both socket/daemon
  entry points.
- `src/tool/factory.rs`, `src/tool/git.rs`, `src/tool/commit.rs`,
  `src/tool/bash.rs`, `src/tool/bash/process.rs` — the publisher is built from
  the daemon's session pool (the same pool the reconciler reads) and the pinned
  Git executable, and is injected into the `git` tool, the `commit` tool, and
  the **bash route**. The bash route matters as much as the two tools: a
  bash-routed `git add` is an authorized `Add`, and without the backend the
  typed executor **refuses** it rather than falling back to the in-place
  route, so an ordinary `git add` in a shell would break. The wiring is
  proven by `the_bash_git_executor_carries_the_publication_backend`.

### Deviations from the plan text

Recorded in full in §9 of the implementation plan. In short: `GIT_INDEX_FILE`
is not used (the `EnvironmentPolicy` deny list refuses it, and the private
gitdir's own index gives the same property); the object import is performed
by the daemon rather than by a child (the mount view makes the repository's
Git metadata read-only to children by design); and the staging step is a
trait so the publication half is provable off a qualified host.

## 4. Verification executed

### Commands run

```bash
cargo check --workspace --locked --all-targets
cargo test -p codegg --locked --test git_index_publication -- --test-threads=1
scripts/verify.sh quick
python3 scripts/check_project_catalog_invariants.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_daemon_cwd_usage.py
cargo fmt --all -- --check
```

### Results

- `tests/git_index_publication.rs`: 22 tests run, 22 passed, 0 failed. That
  count includes the five shared `common::secret_scan` tests the module tree
  brings with it; the C007 tests are the other seventeen. They are the proof
  for the journal, the reconciler, the compare-and-swap, the lock, the object
  import, the atomic replacement, the receipt's network statement, and the
  ordinary dispatch caller.
- The proof suite found two real defects while it was being written, both
  fixed: the v84 migration was wired into the dispatch arms but **not** into
  the `migrate()` upgrade chain, so the table was never created — the
  project-catalog invariant guard caught it (removing the chain entry makes
  the guard fail with
  `storage layout marker tracks highest wired schema migration`), and the
  journal's duplicate-intent refusal was returning a generic storage error
  instead of the typed `Duplicate`.
- `scripts/verify.sh quick`: passed.
- The storage-layout guard compares `STORAGE_LAYOUT_VERSION` against the
  highest wired migration live, so the 83 → 84 bump is checked, not asserted.
- **The full workspace sweep found a real defect in the `Add` arm.**
  `cargo nextest run --workspace --locked --profile ci` reported 6 failures
  out of 14868, all of them the same root cause: the `Add` arm consulted the
  publisher **before** the executor's own policy preflight, so a request the
  policy refused came back as the generic
  `PublicationFailed("the private index publication backend is not
  configured")` instead of the policy's typed outcome
  (`GitMutationError::Policy(BackendUnavailable)`). The affected tests were
  `stage_paths_returns_paths_in_delta`, `stage_all_marks_changes_staged`,
  `git_stage_and_branch_mutation_emit_once_each`,
  `bash_routed_git_mutation_emits_single_git_event`,
  `missing_contained_git_capability_emits_structural_policy_event`, and
  `single_host_trajectory_is_ordered_correlated_and_secret_free`.
  The fix applies the environment policy to the rendered argv at the top of
  `execute_add_publication`, before the publisher is consulted; the rendered
  command is prepared and dropped, so nothing is spawned. This is the plan's
  own §6 requirement — "a non-`Add` mutation is still refused by the same
  preflight" — and the arm was **violating** it, not merely leaving it
  unproven. The sixth failure was a separate trap: `storage_migrations.rs`
  pinned the latest layout version as a literal `83`, which every later
  migration has to move; the pin now reads the declared
  `STORAGE_LAYOUT_VERSION`, and the network-intent migration stays pinned by
  the column appearing over the v81 fixture instead.
  `scripts/verify.sh quick` does **not** run the workspace suite, so this
  class of defect is only caught by the sweep or by CI's `verify` job.
- **The final sweep is clean.** `cargo nextest run --workspace --locked
  --profile ci --no-fail-fast` reports `14869 tests run: 14867 passed, 0
  failed, 2 timed out, 7 skipped`. The two timeouts are
  `tool::lsp::tests::security_context_preset_scenarios_share_one_service` and
  `..._call_expansion_scenarios_share_one_service`; they are **load**-related
  and pre-existing — they appear in the earlier sweep too, they pass in
  isolation (30.4 s each), and the C014 closure already records `3 timed out`
  as this profile's baseline under contention. The order-dependent
  `core::daemon::tests::poolless_standalone_human_shell_uses_registered_workspace`
  flake did not recur in the final sweep and passes in isolation.
- `an_unconfigured_backend_refuses_the_add_mutation` was rewritten rather
  than deleted. The `Add` arm now runs the same preflight as every other
  mutation, and `git add` is classified as requiring contained execution, so
  on a host without that backend the preflight refuses **first** with
  `Policy(BackendUnavailable)` and the missing publisher is never reached.
  The test therefore asserts the property that does not depend on the host:
  the `Add` is refused with a typed error, and the repository index is
  byte-identical afterwards — a refusal, never a silent in-place stage.
- After the v84 DDL was moved inline into `schema.rs` (§3), the gate was
  re-run on the edited tree: `scripts/verify.sh quick` passed again and
  `tests/git_index_publication.rs` still reported 21 passed / 0 failed. The
  storage-layout guard's non-vacuity was re-proved at the same time — deleting
  the `if current_version < 84 { migrate_and_record(pool, 84).await?; }` block
  from the upgrade chain makes it fail with
  `storage layout marker tracks highest wired schema migration` (6/7 checks,
  exit 1), and restoring the block makes it pass 7/7 again.
- **The first hosted run against this source produced no C007 evidence.**
  Run `39313802793` (2026-10-19, push, commit `d03b6267`) was
  **cancelled**: the `process-ipc` job reached its 20-minute timeout with the
  `linux_m004_process_ipc` binary still running, and the cancelled step's log
  was truncated, so it records neither a pass nor a failure for the C007
  test. The cause was the job's budget, not the test: the cold-cache
  workspace build inside that job took 5m14s before the suite started, and
  the suite was still running 14 minutes later.
- **The C007 test is not what overran the job.** Run locally on this host
  (Landlock ABI below 6), the test binary refuses in 0.19 s with a typed
  refusal rather than hanging:
  `Refused(StagingFailed { reason: "the staging child failed to run: managed process sandbox failed: Linux launch dimensions unavailable: process-ipc-landlock-abi-6" })`.
  The test therefore cannot consume the job's budget; the suite around it
  can. The CI defect is corrected in the same branch — the `process-ipc`
  budget goes from 20 to 45 minutes, the `restricted-userns-fails-closed`
  budget from 20 to 35 (it passed with only 2m24s of headroom), and the C007
  proof is moved into its own step with its own 10-minute budget, placed
  **before** the wider suite so a long suite cannot hide its result.
- **The hosted qualification then passed.** Run `39318206124` (2026-10-19,
  push, commit `d024a838`) is a **success** for both qualification jobs.
  The `process-ipc` job's own log is the evidence, not the job badge:
  - the isolated C007 step: `running 1 test` →
    `test production_private_index_publisher_stages_and_publishes_under_containment ... ok`
    → `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 16 filtered out; finished in 0.27s`;
  - the full M004/M005 suite: `running 17 tests` → `test result: ok. 17 passed;
    0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 675.98s`,
    which includes `test managed_process_owner_applies_authenticated_offline_network_intent ... ok`
    (C014's M005 netns owner proof) and the C007 publication test again;
  - the `restricted-userns-fails-closed` job: **success**, with all three
    refusal probes reporting `ok`.

  The filter matches a real test, and the test is not self-skipping: the
  C007 test's skip path prints `skipped: set CODEGG_RUN_M005_OFFLINE_NATIVE=1
  …`, and that line does **not** appear in the log. The 675.98 s the suite
  takes is also the direct explanation for the earlier cancellation.
- **The qualification was re-run at branch HEAD.** Run `39339024314`
  (2026-10-19, push, commit `78359fd3`) is a **success** for both qualification
  jobs and is the run that qualifies the branch's final source. Its log is the
  evidence:
  - the isolated C007 step: `running 1 test` →
    `test production_private_index_publisher_stages_and_publishes_under_containment ... ok`
    → `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 16 filtered out; finished in 0.83s`;
  - the full M004/M005 suite: `running 17 tests` → `test result: ok. 17 passed;
    0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 654.10s`, with
    `test managed_process_owner_applies_authenticated_offline_network_intent ... ok`
    (C014's M005 netns owner proof) and the C007 publication test again;
  - the `restricted-userns-fails-closed` job: **success**, all three refusal
    probes reporting `ok` —
    `required_process_tier_fails_closed_when_user_namespaces_are_restricted ... ok`,
    `m004_daemon_constrained_pty_fails_closed_when_userns_are_restricted ... ok`,
    and `lsp::m004::tests::constrained_lsp_launch_fails_before_target_when_userns_are_restricted ... ok`.

  The non-vacuity check is repeated here rather than assumed from the earlier
  run: the workflow exports `CODEGG_RUN_M005_OFFLINE_NATIVE: 1`, so the gate was
  open, and the literal skip marker `skipped: set CODEGG_RUN_M005_OFFLINE_NATIVE=1`
  appears **zero** times in the job log. The only `skipped:` lines in it belong
  to two different gates — the restricted-userns probe and the C006 Bubblewrap
  qualification. The `process-ipc` job took 34m31s of its 45-minute budget,
  which is why the earlier 20-minute budget cancelled the first attempt.

  This run is the one that covers the two sweep-found defects fixed in
  `78359fd3` — the `Add` arm's own preflight and the bash route's publisher —
  because it executed that commit's exact source.

## 5. Invariant review

- Agent-controlled Git never receives a writable whole-source `.git` mount:
  preserved. The staging child's launch spec is asserted to carry the
  M004/M005 tiers and to protect the repository's Git metadata, and the
  publisher independently verifies that the real index and object database
  are byte-identical after staging.
- External writers are not assumed to cooperate: the compare-and-swap is the
  safety property, and a concurrent writer's index is refused, never
  overwritten.
- Index, working-tree and ref updates are **not** represented as one atomic
  transaction. A single `Add` publishes exactly one resource — the index —
  and the receipt says so (`PublishedResourceScope::IndexOnly`).
- No remote Git, credentials, FullHost fallback, or default policy change
  was added. Merge, rebase, checkout, reset, clean, fetch, push and pull
  are all out of scope and were not touched.
- ADR-0044's status is unchanged, and nothing in this vertical depends on
  its acceptance.

## 6. Failure and recovery review

- A **stale** index (a concurrent writer between staging and publication) is
  a typed `StaleIndex` conflict. The external writer's index survives, the
  lock is released, and no object is imported.
- A **held** `index.lock` is a typed `IndexLockHeld` conflict. The other
  writer's lock is untouched.
- A **crash** before the lock, after the lock, or after the move is
  resolved by the reconciler into exactly one of committed / aborted /
  ambiguous, from the recorded evidence alone. The reconciler never replays
  a committed publication and never guesses an ambiguous one.
- A **failed staging** leaves the repository byte-identical, records nothing
  in the journal, and discards the private artifact.

## 7. Stop conditions

- The CAS against an external writer is **not** vacuous: proven by
  `a_concurrent_external_writer_produces_a_typed_conflict_and_is_not_overwritten`.
- The committed-versus-ambiguous distinction is **not** vacuous: proven by
  the three reconciler tests, each of which reaches a different verdict.
- The atomic replacement within the gitdir filesystem holds: the temporary
  file is written **in** the gitdir, so the lock is taken by a same-filesystem
  hard link and the replacement is a rename within that directory.
- No stop condition is waived by a skipped, feature-gated-off, or
  zero-matching test. The one item that was outstanding — the contained
  staging step on a qualified host — was recorded as **not run** while it
  was outstanding, and it is recorded as a **pass** now only on the hosted
  run's own log lines, which show the test executing (`running 1 test` /
  `1 passed`) and not taking its own skip path.

## 8. Open items

1. **The obtained network fact is not reported.** C014's open item stands.
   The receipt deliberately does not claim an M005 boundary; it states the
   boundary as `not_reported`, which is the truthful answer while the system
   cannot report what it obtained.
2. **`stage_all` and `stage_tracked` still use the legacy in-place route.**
   They stage arbitrary paths, including deletions and whole-tree `-A`
   semantics, which the path-scoped publisher does not model. They are
   outside this vertical and were left unchanged; the inconsistency is
   recorded here rather than papered over.
3. **C005 remains blocked** on accepted ADR-0044, the M005 obtained-facts
   contract, and the supported-host policy. This vertical's own hosted
   qualification is done; C005's remaining gates are not C007's.

## 9. Next steps

- The hosted M004/M005 qualification has run on this branch's exact source and
  passed — first on run `39318206124`, and finally on run `39339024314` at the
  branch's delivered commit `78359fd3`. It is attached to this closure in §4.
- Then C005: progressively restore local Git workflows and qualify the
  lower-cost backend for ordinary use, against the limits recorded above.
