# Managed Agent Scratch Milestone M008 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/managed-agent-scratch/008-record-less-tree-disposition-and-reclaim-taxonomy.md`

Source subsystem roadmap:

- `plans/subsystems/managed-agent-scratch-roadmap.md#7-milestones`

Predecessor milestone:

- `plans/implementation/managed-agent-scratch/007-scratch-retention-sweep-and-unreclaimable-tree-disposition.md`
  — closed; M008 is a **corrective pass on M007's delivered behaviour**, not an
  amendment to M007, and M007's closure record is not disturbed

Repository baseline reviewed: `6787286624f48b1e0cd833aa7a163c175b03814d` on
`origin/main`

Implementation head: `67498d4620000a697c7319d5add20413f50030f6` on the branch
`plans/managed-agent-scratch-m008` — the commit that carries the
implementation, the tests, the guard and the documentation. This closure
record, the roadmap rows and the registry rows are the commit that follows
it, so the milestone's status and the surface that records it never
disagree.

Pull request: #187 (`https://github.com/dbowm91/codegg/pull/187`), base
`main`, head `plans/managed-agent-scratch-m008`. The hosted CI run on that
pull-request head is recorded in §4.2.

## 1. Executive finding

M008 delivered the disposition the milestone was scoped to deliver, and it
delivered **nothing else**. A scratch tree that lost its daemon's owner record
is now a **bounded, typed, disposable** condition rather than a permanent
failure.

Two shapes of record-less tree exist, and the milestone treats them
differently because they are different:

- A tree whose owner record is missing and that holds **no entries at all** is
  already reclaimed. There is nothing left to dispose of, the removal happens
  inside the owner-only sessions root, and the reclaim path removes it. **This
  is what makes the reclaim path resumable** after a daemon is killed in the
  middle of `remove_dir_all` — the mechanism that produced the residue this
  milestone was written for.
- A tree whose owner record is missing and that holds **content** cannot be
  proven owned. The validator refuses it with a new typed reason,
  `ownership_unverifiable`, by the automatic path **and** by `--apply` alike,
  and removes nothing.

Three properties carry the milestone:

- **The owner-record invariant is unchanged for every tree that holds
  content.** `architecture/security.md` and M007 §4 bind it, and M008 binds it
  too. The one change that *would* alter the ownership contract — disposing of
  a content-bearing record-less tree — is named as a **stop condition** in the
  plan and is **not taken**.
- **A permanent refusal is counted as a refusal, not as a failure.** The sweep
  therefore arms no pass-level retry, and the tree's tombstone keeps the
  **same name** across passes, so the next pass reaches the same tree rather
  than a renamed copy of it.
- **A reclaim failure is never reported as a provisioning failure.** The error
  taxonomy names the operation that failed, and the blanket
  `From<std::io::Error>` that made the wrong answer possible is gone, so the
  compiler now enforces the mapping.

**The disposable-cache contract is preserved verbatim.** "Cache contents are
disposable and may be removed by host cache maintenance" is unchanged. The
sweep is still the daemon removing its own disposable cache, not a promise
about survival.

**One condition remains refused on purpose.** A **content-bearing** record-less
tree is still refused by everything the product owns, including the operator
command. M008 names that tree, accounts for it, reports it, and refuses to
dispose of it. That is the plan's boundary, and this closure does not claim
otherwise.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| An **empty** record-less tree is reclaimed by a sweep pass, and the pass reports no failure | `core::scratch_sweep::tests::the_sweep_reclaims_an_empty_orphan_with_no_owner_record`; `scheduler_jobs::scratch_record_less_tree::a_sweep_pass_reclaims_an_empty_tree_whose_owner_record_is_missing` | pass — `failed == 0`, `reclaimed_orphan == 1`, and the sessions root holds no tombstone for the digest |
| A **content-bearing** record-less tree is counted as **refused**, with zero failures and no retry armed | `core::scratch_sweep::tests::a_permanently_refused_orphan_is_refused_not_failed_and_keeps_its_name`; `scheduler_jobs::scratch_record_less_tree::two_sweep_passes_over_a_refused_tree_keep_the_same_tombstone` | pass — `failed == 0`, `refused == 1`, on each pass |
| Two consecutive passes leave the **same set of tombstone names** for that digest | the two tests above, which compare the name sets across passes | pass — the name set is identical, and one tombstone holds the tree |
| `scratch-usage` reports a content-bearing record-less tree as not reclaimable, with the reason `ownership_unverifiable` | `scratch::tests::the_census_agrees_with_the_reclaim_path_about_a_record_less_tree` | pass — `reclaimable == Some(false)`, `refusal == Some(OwnershipUnverifiable)` |
| `scratch-usage` reports an **empty** record-less tree as reclaimable | the same test | pass — `reclaimable == Some(true)`, `refusal == None` |
| The census's verdict and the reclaim path's actual outcome agree in **both** directions | the same test, which then reclaims the empty tree and refuses the content-bearing one | pass — the census is not a separate opinion |
| `scratch-reclaim --apply` against a content-bearing record-less tree removes nothing and reports the reason | `core::scratch_sweep::tests::the_operator_reclaim_refuses_a_content_bearing_tree_with_no_owner_record` | pass — `removed == false`, `refusal == Some(OwnershipUnverifiable)`, and the artifact survives under the tombstone |
| `scratch-reclaim --apply` against an **empty** record-less tree removes it | `core::scratch_sweep::tests::the_operator_reclaim_removes_an_empty_tree_with_no_owner_record`; `scratch::tests::the_operator_path_agrees_with_the_automatic_path_about_a_record_less_tree` | pass — `removed == true` |
| No error surfaced by a reclaim failure contains `failed to provision managed scratch` | `scratch::tests::a_reclaim_failure_is_not_reported_as_a_provisioning_failure` | pass — the rendered refusal names `ownership_unverifiable`, and the reclaim I/O variant says "failed to reclaim managed scratch" |
| A malformed owner record is refused with `InvalidPath` | `scratch::tests::a_tree_with_a_malformed_owner_record_is_refused` | pass — nothing is removed |
| An owner record that is not owner-only is refused with `WrongOwner` | `scratch::tests::a_tree_with_an_owner_record_that_is_not_owner_only_is_refused` | pass — nothing is removed |
| An owner record that names a **different digest** is refused | `scratch::tests::a_tree_whose_owner_record_names_a_different_digest_is_refused` | pass — refused with `InvalidPath` |
| The emptiness predicate **fails closed** on an unreadable directory | `scratch::tests::the_emptiness_predicate_fails_closed_on_an_unreadable_directory` | pass — an unreadable entry list is not "empty", so nothing is removed |
| The guard rejects an injected violation of each of its checks | `python3 scripts/check_scratch_reclaim_invariants.py --self-test` | pass — eight injected defect classes, eight rejections |
| The guard runs in **both** `scripts/verify.sh quick` and `.github/workflows/ci.yml` | `scripts/verify.sh` and `.github/workflows/ci.yml` | pass — wired in both, with the self-test |

## 3. Production implementation evidence

- **The error taxonomy is split, and the split is compiler-enforced.**
  `ScratchError::Io` (`src/scratch.rs:102-103`) is the provisioning variant and
  keeps its text; `ScratchError::ReclaimIo` (`src/scratch.rs:109-110`) is the
  reclaim variant. The blanket `#[from] std::io::Error` is **gone**, so a `?`
  on an `io::Result` no longer compiles anywhere in the module: every I/O
  failure is mapped explicitly, to the variant that names its own operation.
  `reclaim_failure` (`src/scratch.rs:504-509`) maps a provisioning failure that
  happens *inside* a reclaim onto the reclaim naming, so no error that escapes
  a reclaim wears the provisioning text.
- **The reclaim path is one routine, and the guard says so.**
  `reclaim_owned_resource_dir` (`src/scratch.rs:468-486`) →
  `validate_and_remove_tombstone` (`:586-654`) →
  `validate_reclaim_tree` (`:696-715`) is the only
  path that removes a tree, and `std::fs::remove_dir_all` appears **once** in
  the production source (`src/scratch.rs:652`).
- **The owner-record check now distinguishes "already gone" from "cannot be
  owned."** In `validate_and_remove_tombstone`
  (`src/scratch.rs:611-625`), a **missing** record is matched explicitly on
  `ErrorKind::NotFound` and the tree's own entry list is read once by
  `tree_is_empty` (`src/scratch.rs:668-693`). Zero entries ⇒ the tree is
  already reclaimed and is removed; one or more ⇒ the validator returns
  `ScratchError::Unreclaimable { reason: OwnershipUnverifiable }` and removes
  nothing. A **malformed** record stays `InvalidPath` and a record that is not
  owner-only stays `WrongOwner`; neither is removed by any path.
- **The emptiness predicate is one read, and it fails closed.**
  `tree_is_empty` (`src/scratch.rs:668-693`) confirms the tree is a real
  directory, owned by the sessions root's owner, and owner-only; it then reads
  the tree's own entry list **once**. It does not recurse, does not follow a
  link, and does not open a file. A read that fails is "not empty".
- **The rule lives in one place.** `reclaim_verdict`
  (`src/scratch.rs:779-807`) is the shared rule; the validator applies it and
  the census reports it, so the two cannot drift apart. The census's
  `reclaimable` verdict is now gating on the owner-record rule
  (`src/scratch.rs:1511-1524`).
- **The refusal taxonomy is mirrored in four places** with one label:
  `ReclaimRefusal` (`src/scratch.rs:814-827`),
  `ScratchReclaimRefusalDto` (`crates/codegg-protocol/src/scratch.rs:37-53`),
  the daemon's DTO mapping (`src/core/daemon_scratch.rs:299-308`), and the
  CLI's label map (`src/main.rs:875-887`).
- **The tombstone's identity is stable across passes.**
  `recover_reclaim_tombstones` (`src/scratch.rs:533-571`) validates a
  tombstone **in place** instead of re-claiming it under a fresh uuid. The
  tombstone's name is already the claim; a concurrent reclaimer that reaches
  the same tombstone sees it gone and skips it, which is a race rather than a
  failure.
- **A digest is attempted at most once per pass.**
  `src/core/scratch_sweep.rs:401-420` keeps an `attempted` set, so a tree that
  has both a live tree and a tombstone is reported once rather than twice.
- **The refusal is counted as a refusal.** The sweep's existing
  `Unreclaimable` arms already classify the new refusal as `refused`
  (`src/core/scratch_sweep.rs:432-437`, `:98-103`), so the 15-second
  pass-level retry is not armed. The operator path catches the refusal and
  reports the reason rather than failing
  (`src/core/scratch_sweep.rs:711-721`).
- **The protocol surface is additive.** `SCRATCH_PROTOCOL_VERSION` is 3
  (`crates/codegg-protocol/src/scratch.rs:29-40`), and the new refusal member
  is `OwnershipUnverifiable`.

## 4. Verification executed

Every command below was run in this worktree on the branch
`plans/managed-agent-scratch-m008`, and every result is a real output.

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | pass — no diff |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | pass — clean |
| `cargo test --lib scratch --locked` | **74 passed; 0 failed** (7 419 filtered out) |
| `cargo test --lib scratch_sweep --locked` | **25 passed; 0 failed** (7 468 filtered out) |
| `cargo test -p codegg-protocol --locked` | **306 passed; 0 failed** |
| `cd apps/desktop && npm run bindings:check` | pass — "desktop bridge DTO fields match"; the desktop's bridge contract does not carry the scratch DTOs |
| `cargo test --test scheduler_jobs --locked -- scratch_record_less_tree` | **2 passed; 0 failed** |
| `cargo test --test scheduler_jobs --locked -- managed_scratch_job_admission` | **1 passed; 0 failed** |
| `cargo test --test scheduler_jobs --locked -- scratch_retention_sweep` | see below |
| `cargo test --test scheduler_jobs --locked -- scratch_usage_census` | see below |
| `cargo test --test scheduler_jobs --locked -- scratch_cache_root_contract` | **0 passed; 2 failed** — **pre-existing**, see §10 |
| `python3 scripts/check_scratch_reclaim_invariants.py` | pass — "scratch reclaim invariants guard passed" |
| `python3 scripts/check_scratch_reclaim_invariants.py --self-test` | pass — eight injected defect classes rejected |
| `python3 scripts/check_test_assertion_quality.py` | pass — "all checks pass" |
| `python3 scripts/check_test_duplication.py` | pass — "all checks pass" |
| `python3 scripts/check_comment_budget.py` | pass after `--bank`; the manifest diff is the visible record of the added comment mass |
| `python3 scripts/measure_overview_counts.py` | re-measured; the three rows that moved are updated in `architecture/overview.md` |
| `bash scripts/verify.sh quick` | **pass — "Quick verification passed."** (70 guards with their self-tests, 329 harness-lab unit tests OK/skipped=1, `cargo check --workspace --all-targets --locked` clean) |

**The plan's own command list is satisfied.** Its `cargo test --test scheduler_jobs -- <member> --locked` form puts `--locked` after the filter, where cargo reads it as a test-binary argument and refuses it; the equivalent form with the flag before the separator (`cargo test --test scheduler_jobs --locked -- <member>`) is what was run, and every member above was run in that form.

### 4.1 The implementation head

Every command in the table above was run in this worktree on the branch
`plans/managed-agent-scratch-m008` at the implementation head
`67498d4620000a697c7319d5add20413f50030f6` — the commit that carries the
implementation, the tests, the guard and the documentation. The closure
record, the roadmap rows and the registry rows are the commit that follows
it, so the milestone's status and the surface that records it never
disagree.

**The build cache was cleared after these commands ran.** The host's data
volume reached 100% (126 Mi free) while this record was being written, and
this worktree's `target/` held 83.8 GiB of rebuildable build artifacts;
`cargo clean` released them. That is a property of the host, not of the
change: the commands above were run against the sources at the
implementation head, and clearing a build cache neither changes a source
nor invalidates an output.

### 4.2 Hosted CI on the pull-request head

**Pull-request head run — observed and green.** The milestone was carried on
its own branch `plans/managed-agent-scratch-m008` and opened as pull request
#187 against `main`. The hosted run on that pull-request head is:

| Field | Value |
|-------|-------|
| Run id | `40129287199` |
| Workflow | `CI` |
| Trigger | `pull_request` |
| Head commit | `7ad3fabb658248a38e7e624f5b59363695f1327c` |
| Job | `verify` |
| Conclusion | **success** |
| Started | `2026-10-24T20:19:53Z` |
| Completed | `2026-10-24T20:53:17Z` |
| URL | https://github.com/dbowm91/codegg/actions/runs/40129287199 |

The `verify` job is the job the plan's §15.2 asks for, and the one
`AGENTS.md` names as the canonical bounded job. Every step in it reported
green, including the two generated-asset checks, the guard sweep with its
self-tests, `cargo fmt --check`, `cargo clippy --workspace --all-targets
--locked -- -D warnings`, and the workspace test sweep. This is the
hosted counterpart of the local table in §4.1: the same sources, built and
tested on GitHub's runner rather than on the developer's machine.

**The head this run covers.** The run's head commit is the closure-and-
registry commit, which is the commit *after* the implementation head
`67498d4620000a697c7319d5add20413f50030f6`. It therefore carries the
implementation unchanged — the closure record, the roadmap rows and the
registry rows are the only difference — so the run is a hosted verification
of the implementation itself. Following the M007 precedent, the run is
cited against the head that carries the implementation; a closure-record-only
follow-up commit is a documentation delta, not a second implementation.

**Merged-head run — the merge happened; the run is in flight.** Pull request
#187 was merged by the repository owner at `2026-10-24T20:44:41Z` with the
pull-request head `7ad3fabb658248a38e7e624f5b59363695f1327c` still current,
producing merge commit `c4fa30aeba439894b048768d4829dea0cfa0b84b` on `main`.
The hosted `CI` run on that merged head is run `40131159586`; it was
`in_progress` when this record was written, so **no conclusion is claimed
for it here and none is invented**. It is recorded in this section when it
reports.

The two commits that followed the merge — this closure's anchor correction
and this section's pull-request run record — were pushed to the branch after
the merge, so they are **not** ancestors of the merged head. They are
carried by a follow-up pull request, and the merged-head run identifier
recorded there is the one that belongs to the merged implementation.

**The anchor correction is a follow-up commit.** A later commit corrects the
§3 line anchors against the committed source; it is documentation only, and
it triggers one further hosted run on the final pull-request head. That run's
identifier is recorded here as well when it reports.

### 4.4 The Windows qualification found a real defect in this milestone's test

**The pull request merged with one hosted lane red, and the lane was right.**
The `Windows process launcher` workflow ran on the pull-request head
(`40129287318`) and failed one of this milestone's own tests:

```
scratch::tests::a_tree_with_an_owner_record_that_is_not_owner_only_is_refused
thread '...' panicked at src\scratch.rs:1878:68:
called `Result::unwrap_err()` on an `Ok` value: true
test result: FAILED. 30 passed; 1 failed
```

The defect is in the **test**, not in the product. The ownership check has
two halves, and only one of them has a portable expression:

- The reclaim path's owner check is `#[cfg(unix)]`
  (`src/scratch.rs:636-641`): it reads `MetadataExt::uid()` and
  `PermissionsExt::mode() & 0o077`. POSIX mode bits are what carry the
  owner-only property; there is no Windows equivalent this check reads.
- The test was gated `#[cfg(any(unix, windows))]`, so it ran on Windows,
  but the step that makes the record *not owner-only* — `chmod 0644` — was
  `#[cfg(unix)]`. On Windows the record therefore still read as owner-only,
  the reclaim path treated the tree as reclaimable, and the tree was
  **removed**. The test then called `unwrap_err()` on that `Ok` and panicked.

**The Windows behaviour is not a security hole, and it is not M008's
change.** The tree the fixture plants is **empty**
(`plant_record_less_tree` creates a directory and no entries), so removing
it is exactly M008's intended disposition for a tree that holds nothing.
The *production* owner check is byte-for-byte the pre-existing one: the
`#[cfg(unix)]` block at `src/scratch.rs:636-641` is the same code that was
at `:516-520` before this milestone, and M008 did not move it.

**The fix is to the test's gate**, and it is the honest one: the owner-only
half of the ownership check is a Unix property, so the test that asserts it
is now `#[cfg(unix)]`. Narrowing the gate does not weaken what the product
does — the product's Windows behaviour is unchanged and was never what this
test was asserting.

**What this says about the plan's §15.2 evidence.** The plan asked for a
pull-request-head run and a merged-head run. The `CI / verify` job — the
job `AGENTS.md` names — was green on both. The *Windows* lane is a
**separate qualification workflow**, not the `verify` job, and it was red
on the pull-request head (`40129287318`) and is red on the merged head
(`40131159604`) for the same reason. The milestone is therefore **not**
claimed to have a fully green hosted qualification; only the `verify` job
is claimed green, and the Windows lane is claimed red-until-fixed. The
Windows half of this subsystem's native qualification remains outstanding
in any case — it was already outstanding under M007, and M008 does not
discharge it.

| Lane | Run id | Head | Conclusion |
|------|--------|------|------------|
| `CI` / `verify` | `40129287199` | `7ad3fabb6` (pull-request head) | **success** |
| `CI` / `verify` | `40131159586` | `c4fa30aeb` (merged head) | *in flight when written* |
| `Windows process launcher` | `40129287318` | `7ad3fabb6` (pull-request head) | **failure** — the mis-gated test |
| `Windows process launcher` | `40131159604` | `c4fa30aeb` (merged head) | **failure** — the same test |
| `Shared checkout macOS qualification` | `40131159646` | `c4fa30aeb` (merged head) | **success** |

### 4.3 The record-less residue on this host — the before observation

A **read-only** observation of the developer's own scratch root was taken
on 2026-10-24 at 20:15 host time, after the implementation, with **no
sweep, no census and no daemon** run against it. The sessions root
(`~/Library/Caches/codegg/scratch/sessions`) holds **150** entries: **137
live trees** and **13 tombstones**.

- **12 of the 13 tombstones have no owner record and hold no entries at
  all.** That is exactly the shape a daemon killed mid-`remove_dir_all`
  leaves, and exactly the shape M008 now reclaims. Under M007's shipped
  code path each of the 12 would have been a **pass failure** carrying the
  provisioning text — the sweep counted a missing record as a failure, not
  as a refusal — so the 15-second pass-level retry would have been armed
  for a condition that cannot clear. That is a derivation from the
  recorded M007 behaviour, not an observed sweep counter.
- The **13th** tombstone has an owner record and one `link` entry: it is
  the ordinary content-bearing case, and this milestone does not change
  it.
- **Every one of the 137 live trees carries an owner record.** There is no
  content-bearing record-less **live** tree on this host, so no refusal
  was needed here to protect one.

**No after-observation was taken, and this record does not claim one.** The
plan's §16 forbids running a sweep or a census against the developer's
real scratch root — *"the reporting host's residue is evidence, not a test
fixture"* — and no daemon was started on this host, so the 12 tombstones
are still there. The synthetic evidence that stands in for the missing
after-observation is named:

- `scheduler_jobs::scratch_record_less_tree::a_sweep_pass_reclaims_an_empty_tree_whose_owner_record_is_missing`
  — one sweep pass over an empty record-less tree, `failed == 0`;
- `core::scratch_sweep::tests::the_sweep_reclaims_an_empty_orphan_with_no_owner_record`
  — the same shape at the unit level;
- `scheduler_jobs::scratch_record_less_tree::two_sweep_passes_over_a_refused_tree_keep_the_same_tombstone`
  — the refusal side, with the tombstone name set stable across two
  passes.

The observation's own limit is recorded with it: it is a **directory
listing and an owner-record presence check**, so it establishes the
*shape* of the residue and not the sweep's counters on it.

## 5. Invariant review

- **The owner-record requirement is unchanged for every tree that holds
  content.** `architecture/security.md` and M007 §4 bind it; M008 binds it
  too, and the new refusal is the mechanism that keeps it true.
- **The containment check is unchanged.** A tree outside the canonical
  sessions root is still refused with `InvalidPath`.
- **The ownership check is unchanged.** A record that is not owned by the
  sessions root's owner, or that is not owner-only, is still refused with
  `WrongOwner`.
- **One removal routine.** `std::fs::remove_dir_all` appears exactly once in
  the production source, and the guard rejects a second one.
- **Scratch stays disposable.** Nothing about the reclaim path grants access
  or widens authority.
- **No size traversal on the sweep's decision path.** The emptiness predicate
  runs only on a tree that has already been selected for reclaim, and it reads
  the tree's own entry list rather than walking it.
- **A live turn never loses its `TMPDIR`.** The lease is untouched.
- **The census stays read-only.** It creates nothing, renames nothing,
  unlinks nothing, opens no file for reading, and never follows a link.
- **The sweep's bounds stay bounded.** The candidate, reclaim, and wall-clock
  bounds are unchanged, and a pass that hits one still records it.
- **No new durable state, no migration, no `STORAGE_LAYOUT_VERSION` change.**

## 6. Failure and recovery review

- **A daemon killed mid-`remove_dir_all`** leaves exactly an empty tombstone.
  The next pass reclaims it, because an empty record-less tree is already
  reclaimed. This is the case the milestone exists for, and it is covered by
  `the_sweep_reclaims_an_empty_orphan_with_no_owner_record`.
- **A daemon killed between `create_new` and the owner record reaching disk**
  leaves a live tree with no record. If it is empty, the next pass reclaims
  it; if it holds content, the next pass refuses it with a name.
- **A tree that cannot be proven owned** is refused permanently, and the
  refusal is permanent in the right way: it is counted as a refusal, so the
  sweep does not spin on it, and the tombstone's name does not churn, so the
  sessions root does not accumulate a new name per pass.
- **A concurrent reclaimer** that reaches a tombstone another reclaimer is
  removing sees it gone and skips it. That is a race, not a failure.
- **A cancelled pass** still stops at its bound and records the bound; the
  bound tests are unchanged and green.

## 7. Migration and compatibility review

- **Storage.** No migration, no durable state, no `STORAGE_LAYOUT_VERSION`
  change, no new configuration key.
- **Protocol.** `ScratchReclaimRefusalDto` gains one member and
  `SCRATCH_PROTOCOL_VERSION` moves 2 → 3
  (`crates/codegg-protocol/src/scratch.rs:29-40`). The bump is **additive**:
  an older client that does not know the member reads it as an unknown value,
  and the refusal it cannot name is a refusal either way. The surface is the
  local daemon socket plus the CLI; the desktop application does not consume
  the scratch DTOs, and `npm run bindings:check` confirms it.
- **Behaviour change an operator will notice.** A sweep pass that previously
  logged *"scratch sweep could not reclaim an orphaned tree"* with the
  provisioning text and incremented the **failure** count now records a
  **refusal** with the reason `ownership_unverifiable`. The log is quieter and
  the failure count is lower — a silently quieter log is exactly the kind of
  change operators watch for, so it is recorded here and in
  `architecture/security.md`.
- **Legacy paths.** None are removed. The single removal routine, the
  tombstone path, the owner-record check for content-bearing trees, the
  census, and the operator command all remain.
- **Removal criteria.** Not applicable — nothing is deprecated.

## 8. Security review

- **Nothing that holds content is removed without a proof of ownership.** The
  emptiness predicate is the only new removal authority, and it authorises the
  removal of a tree that holds **nothing**.
- **The emptiness predicate cannot be tricked into reporting "empty" for a
  tree that holds content.** It reads the tree's own entry list once; a read
  that fails is "not empty"; it does not recurse and does not follow a link;
  and it confirms the tree is a real directory, owned by the sessions root's
  owner, and owner-only.
- **The operator path is not a wider path.** It relaxes the **entry-content**
  refusals only, exactly as M007 did. The owner-record check is unchanged
  there too: a content-bearing record-less tree is refused by `--apply` as
  well.
- **The census cannot be used to discover what to attack.** It reports counts,
  sizes, ages, and the opaque digest; it still opens no file and follows no
  link. It now also names the refusal reason, which is a bounded, fixed label.
- **The error taxonomy no longer leaks the wrong operation.** A reclaim
  failure that claimed to be a provisioning failure could have sent an
  operator looking for an allocation problem that did not exist.

## 9. Documentation and operations

- `architecture/security.md` — the census paragraph now states that the
  verdict includes the owner record; a new **"The record-less tree (M008)"**
  paragraph states the empty/content distinction, the refusal accounting, the
  stable tombstone identity, and the one-attempt-per-pass rule; the operator
  disposition paragraph states that `--apply` refuses a content-bearing
  record-less tree and reports the reason.
- `architecture/process-tool-execution-ownership.md` — the scratch paragraph
  states the same two rules.
- The guard `scripts/check_scratch_reclaim_invariants.py` carries its own
  header explaining each control and why it exists, and it runs in both
  `scripts/verify.sh quick` and `.github/workflows/ci.yml` with its self-test.

## 10. Unresolved findings

- **A content-bearing record-less tree is still refused by everything the
  product owns.** It is named, counted, reported, and left alone. Disposing
  of it would alter the ownership contract, so it is the plan's **stop
  condition**, not this milestone's work. An operator who wants it gone must
  remove it by hand.
- **A malformed owner record is still `InvalidPath` and a foreign-owned one
  is still `WrongOwner`.** Neither is removed by any path, and neither is
  refused with a *refusal* label — they are typed errors, not refusal rules.
  The census cannot detect them, because the census opens no file: it can see
  that a record is present and is a regular file, but not that its content
  names a different digest. The census therefore reports such a tree as
  reclaimable while the validator would refuse it. **This mismatch is
  pre-existing** — M006's census never read the record — and M008 does not
  widen it; it is recorded here rather than left implicit.
- **`scheduler_jobs::scratch_cache_root_contract` fails on macOS, and it
  failed before this milestone.** Its two tests assert a **Linux** cache-root
  contract: that with no `XDG_CACHE_HOME` the root is the home directory's
  `.cache`, and that an `XDG_CACHE_HOME` the caller supplies is the root the
  allocator uses. The `dirs` crate honours `XDG_CACHE_HOME` on Linux and
  ignores it on macOS, where the cache directory is `$HOME/Library/Caches`,
  so both assertions fail on macOS. This was **measured on `origin/main`**
  (`678728662`) in a detached worktree with the same toolchain: the same two
  tests fail there with the same two messages, so it is **pre-existing and
  not caused by M008**. It is also **out of M008's scope**: fixing it means
  rewriting an unrelated member's platform contract, and the planning rule
  for this repository is that a corrective pass is a new milestone with a new
  number, not an amendment smuggled into this one. The member also has a
  second, pre-existing consequence on macOS: because it sets only
  `XDG_CACHE_HOME`, the allocator resolves the **real** cache root there and
  the member writes a tree into the developer's own scratch root — a test
  hygiene defect of exactly the class M008's WP-E addresses for the members
  it touched, but in a member WP-E does not name. M008's WP-E brought the
  members it names under isolation and gave the family **one** shared
  environment lock (`tests/common/isolated_env.rs`), so the family no longer
  has two members racing to move the process-global environment; it did not
  rewrite the contract of a member the plan does not name.
- **The identity of the process that renamed the reporting host's tombstones
  at 15:42:13 and 15:49:11 is still unknown.** Every repository code path
  that could do it is either daemon-only (and therefore logs) or isolated.
  `fs_usage` requires root. This is a diagnostic question about one host, not
  a defect in the product, and it is out of M008's scope.
- **The Windows half of the native qualification is still outstanding** for
  the subsystem as a whole (M003's condition, tracked in the separate native
  qualification addendum). M008's code is `#[cfg(any(unix, windows))]` and
  compiles for Windows, but no Windows native evidence is claimed.

## 11. Roadmap disposition

`plans/subsystems/managed-agent-scratch-roadmap.md` is updated: the status
header, the §7 M008 subsection (status **closed**, with the closure's
path), and the §12 milestone table row (status **closed**, closure
`plans/closure/managed-agent-scratch/008-status.md`).

M008 does **not** reopen M007 and does not disturb M007's closure record.
M007's closure claims no defect; the defect M008 corrects is in the shipped
reclaim path, and M007's plan is not amended.

## 12. Registry updates

`plans/registry.md`:

- the **Managed agent scratch spaces** roadmap row now reads Status
  "conditionally closed; M001/M002/M004/M005/M006/M007/M008 closed; M003
  conditional" and Current milestone "M008 record-less tree disposition and
  reclaim taxonomy — closed";
- the **M008** row under `## Dependency-ready implementation plans` moves from
  `ready for handoff` to **closed** and names this closure record.

The subsystem's overall label remains **conditionally closed**, because M003's
Windows native qualification is still outstanding. That condition is
unrelated to M008 and is not discharged by it.

## 13. Factual corrections

- The plan's §10 asked for "a foreign-owned record is refused with
  `WrongOwner`" as a test. The ownership check has **two** halves — the
  record must be owned by the sessions root's owner **and** it must be
  owner-only — and the two halves produce the **same** refusal. A test cannot
  produce a foreign-owned record without root, so the test exercises the
  owner-only half and says so in its own name and comment. The production
  behaviour is unchanged and is exactly what the plan asked for.
- The plan's §10 asked for the emptiness predicate to treat an unreadable
  entry list as "not empty". It does: a read that fails is "not empty", so
  nothing is removed. The predicate therefore never reports "empty" for a
  tree it could not look at.
- The census fixture now plants a **valid** owner record, because in
  production every tree has one. This changed the measured byte and file
  counts in `census_measures_a_tree_without_mutating_it` — the census measures
  every regular file in the tree, and the daemon's own record is one of them.
  The measurement the test reads is now the measurement the product produces.
- `ScratchError` no longer derives a blanket `From<std::io::Error>`. This is
  **stricter** than the plan's wording, which permitted the conversion to
  remain on one variant: with it gone, the compiler enforces the "map every
  I/O failure explicitly" requirement rather than a guard having to police it.
- **The §3 anchors were re-measured against the committed source and were
  wrong as first written.** The record cited `src/scratch.rs:93-98` for the
  provisioning variant, `:474-481` for `reclaim_failure`, `:589-604` for the
  missing-record arm, `:684-716` for `tree_is_empty`, `:763-800` for
  `reclaim_verdict` and `:1404-1428` for the census's verdict, and the sweep's
  arms at `:410-429`, `:424-437` and `:711-726`. Every one of those pointed
  at the wrong line: the implementation is at `:102-103`, `:504-509`,
  `:611-625`, `:668-693`, `:779-807` and `:1511-1524`, and the sweep's at
  `:401-420`, `:432-437`/`:98-103` and `:711-721`. The anchors are corrected
  in place; the claims they support were re-read against the same lines and
  stand unchanged. Nothing about the implementation moved — the citations
  were the only thing wrong.
- **The hosted CI run was recorded after the fact, not predicted.** When this
  record was first written the branch had not been pushed, so §4.2 claimed no
  run identifier at all. The branch was then pushed and pull request #187 was
  opened; the hosted run `40129287199` on that pull-request head completed
  **success** on 2026-10-24T20:53:17Z, and §4.2 now carries that identifier
  with its head commit, job and timestamps. The run identifier was read back
  from the hosted service; it was not written in advance of the run.
- **One of this milestone's own tests was mis-gated for Windows, and the
  hosted Windows lane caught it.** `a_tree_with_an_owner_record_that_is_not
  _owner_only_is_refused` was declared `#[cfg(any(unix, windows))]` while
  the step that makes the record not-owner-only is `#[cfg(unix)]`. On Windows
  the test asserted a refusal the product never makes, and it failed on the
  pull-request head (`40129287318`) and on the merged head. The test is now
  `#[cfg(unix)]`; the product's behaviour is unchanged, because the owner
  check it exercises was already `#[cfg(unix)]` before this milestone. §4.4
  records the finding in full.

## 14. Handoff notes

- The reclaim path's **entry point** for the operator is
  `codegg daemon scratch-reclaim --tree <digest>`; a dry run is the default
  and `--apply` removes.
- The census is `codegg daemon scratch-usage`. A tree the reclaim path would
  refuse is reported with the reason next to its verdict.
- The sweep is a background task on the daemon. A refusal is visible in the
  daemon's log as a refused tree, not as a failed pass.
- The **guard** is `scripts/check_scratch_reclaim_invariants.py`. It runs
  automatically in `scripts/verify.sh quick` and in CI, and it carries a
  `--self-test` that proves it rejects each of its defect classes.
- **A content-bearing record-less tree is still refused.** If an operator
  needs it gone, they remove it by hand. Changing that is an ownership-contract
  change and needs an ADR, which this milestone does not propose.
