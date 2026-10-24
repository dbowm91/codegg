# Managed Agent Scratch Milestone 008 — Record-less tree disposition and reclaim taxonomy

Status: ready for handoff

Repository baseline: `6787286624f48b1e0cd833aa7a163c175b03814d` (`origin/main`)

Source roadmap:

- `plans/subsystems/managed-agent-scratch-roadmap.md#7-milestones`

Long-term requirements:

- `plans/000-long-term-specification.md` §§4.2–4.4, 16–18, 24, 26–29 (ownership, paths as locators, daemon authority, reliability and security)

Applicable ADRs:

- none. No ADR governs scratch today; the binding rules live in `architecture/security.md` §"Operator disposition (M007)" (`architecture/security.md:626-640`) and `architecture/process-tool-execution-ownership.md:35-49`. **This milestone must not change the ownership contract.** If the implementation concludes it must, §14 requires it to stop and propose an ADR.

Primary class: invariant

Corrective-pass note: this is a **corrective pass on M007**, not an amendment to it. It is a new plan in the same subsystem directory with a new number, per `plans/implementation/README.md` §"Corrective plans" (`:174-181`). The defect it corrects was found in M007's delivered behaviour, not in M007's plan; M007's closure record (`plans/closure/managed-agent-scratch/007-status.md`) claims no defect, so this plan does not reopen M007 and does not disturb its closure.

---

## 1. Objective

**Make a scratch tree that lost its owner record a bounded, typed, disposable condition instead of a permanent, misreported failure.**

Today a tree whose owner record is missing cannot be reclaimed at all:

- the reclaim validator fails on the **missing** record with an `io::Error` that the error taxonomy renders as *"failed to provision managed scratch"* (`src/scratch.rs:92-93`), so a reclaim failure is reported as a provisioning failure;
- the sweep counts that as a **pass failure** (`src/core/scratch_sweep.rs:430-437`), which arms the 15-second pass-level retry (`src/core/scratch_sweep.rs:126`, `:762-770`) for a condition that is permanent;
- the orphan reclaim path re-parks the tombstone under a **fresh uuid before** validating it (`src/scratch.rs:472-473`), so the tombstone's name churns on every pass and the digest is blocked forever — the live tree for that digest is never reached, because the error propagates out of `reclaim_owned_resource_dir` before the live tree is even renamed (`src/scratch.rs:428`);
- the usage census reports such a tree as **reclaimable**, because the census's `owner_record_present` is non-gating (`src/scratch.rs:1166`), so the dry run misreports a tree that `--apply` will refuse;
- the operator command cannot dispose of it either: `ReclaimStrictness` branches only inside `validate_reclaim_tree` (`src/scratch.rs:539-541`) and never reaches the owner-record check (`src/scratch.rs:511-529`), so `--apply` removes nothing and reports the provisioning text.

The proven mechanism for how a tree loses its record is in §3 F4. M008 delivers five things:

1. an **error taxonomy that names the failure** — a reclaim failure is never reported as a provisioning failure;
2. a **distinction between "already gone" and "cannot be owned"** — an **empty** tree with no owner record is already reclaimed and is removed; a tree with **content** and no owner record is refused with a **new typed refusal** (`ownership_unverifiable`), never removed;
3. **bounded refusal accounting** — a permanent refusal is counted as `refused`, not `failed`, so it arms no pass-level retry, and the tombstone's identity is stable across passes;
4. a **census that agrees with the validator** — `scratch-usage` never reports a tree as reclaimable that the reclaim path will refuse;
5. **regression evidence and a static guard** — the defect class is caught by a test that exercises the sweep, and the two static invariants it depends on are enforced by a guard with a `--self-test`.

The milestone is bounded: it changes the daemon's own disposable scratch handling and its error reporting, and it changes nothing else.

## 2. Why this milestone is ready

Hard dependencies, all closed:

- **M007** (`plans/closure/managed-agent-scratch/007-status.md`) — the retention sweep, the orphan rule, and the operator disposition are closed and shipped. M008 corrects M007's *delivered behaviour*; it does not depend on any M007 work still being in flight.
- **M006** (`plans/closure/managed-agent-scratch/006-status.md`) — the census is implemented and is the surface whose verdict must be corrected.
- **M004** (`plans/closure/managed-agent-scratch/004-status.md`) — the owner-validated tombstone reclaim path is stable and is the **only** removal routine M008 may use.

Stable interface dependencies:

- the daemon-socket request surface (`src/core/daemon_scratch.rs:126-140`) and the CLI lifecycle-command surface (`src/main.rs:723-736`) are shipped and are extended, not replaced;
- `ReclaimStrictness` (`src/scratch.rs:592-600`) already exists and already distinguishes the automatic path from the operator path — M008 **reuses** it and does **not** introduce a second removal routine;
- the protocol crate's scratch surface (`crates/codegg-protocol/src/scratch.rs`) is a local-daemon-socket and CLI surface only; the desktop application does not consume the scratch DTOs.

There is **no** open design decision. §5's out-of-scope list is what keeps the milestone one coherent pass.

## 3. Current implementation evidence

Anchors are at the repository baseline above.

### F1 — The owner-record check propagates a raw I/O error into the provisioning variant

`src/scratch.rs:511-512`:

```rust
let owner_record = tombstone.join(OWNER_RECORD_NAME);
let owner_metadata = std::fs::symlink_metadata(&owner_record)?;
```

The `?` converts `std::io::Error` into `ScratchError::Io` through `#[from]` (`src/scratch.rs:92-93`):

```rust
#[error("failed to provision managed scratch: {0}")]
Io(#[from] std::io::Error),
```

Every I/O failure in the module therefore wears the text *"failed to provision managed scratch"*, including reclaim validations where **nothing was provisioned**. A missing owner record is `ErrorKind::NotFound`; the message an operator sees is *"failed to provision managed scratch: No such file or directory (os error 2)"*.

### F2 — The sweep treats a permanent condition as a transient failure

The orphan arm (`src/core/scratch_sweep.rs:421-437`) matches only `ScratchError::Unreclaimable` as a refusal; every other error increments `report.failed` and logs a warning. A failed pass arms the 15-second retry (`SWEEP_RETRY_SECONDS = 15`, `src/core/scratch_sweep.rs:126`) for up to `SWEEP_MAX_CONSECUTIVE_RETRIES = 3` consecutive passes (`src/core/scratch_sweep.rs:133`, `:762-770`). There is no backoff, no give-up, and no escalation: the condition is retried forever, once per interval.

The session arm has the same shape (`src/core/scratch_sweep.rs:374-382`), keyed on `SweepError::is_refusal` (`src/core/scratch_sweep.rs:98-103`), which matches `ScratchError::Unreclaimable` only.

### F3 — The tombstone is re-parked under a new name on every pass, and the failure blocks the live tree

`recover_reclaim_tombstones` (`src/scratch.rs:450-489`) runs **before** the live tree's own reclaim (`src/scratch.rs:428`, inside `reclaim_owned_resource_dir` at `:419-436`). For each tombstone it renames the tombstone to a fresh `.reclaim-<digest>-<uuid>` (`:472-473`) and **then** validates it (`:475-481`). The validation error propagates out of the loop and out of `reclaim_owned_resource_dir`, so:

- the tombstone's name changes on every pass, and the error escapes before the *live* tree for that digest is ever renamed (`:430-433`) — so a live tree sharing the digest is blocked too;
- the digest is permanently abandoned.

Observed on the reporting host: one digest (`da8cd6c50`) had **both** a live tree and a tombstone and was reported **twice within one pass** (`daemon.log` 15:28:01.097857 and .098153).

### F4 — The proven mechanism: a daemon killed mid-`remove_dir_all`

`validate_and_remove_tombstone` removes the tree with `std::fs::remove_dir_all(tombstone)` (`src/scratch.rs:531`). `remove_dir_all` removes the children first and the directory last. A daemon killed between those two steps leaves **exactly an empty tombstone** — no owner record, no entries.

The allocation path already anticipates this class: `ensure_owner_record` (`src/scratch.rs:783-847`) handles "a daemon may stop after `create_new` but before the complete owner record reaches disk" by removing a truncated record (`:831`) and rewriting it (`:839-841`). That recovery is reachable **only** from the allocation path (`:218`, `:386-387`). The orphan path never calls it, so it can never repair a record it finds missing.

### F5 — The census's `owner_record_present` is non-gating

`src/scratch.rs:1166-1169` records `owner_record_present` as a *fact*. The `reclaimable` verdict is computed from the entry-content walk alone, so a tree with **no owner record** and **no** offending entry is reported `reclaimable: Some(true)` — while `reclaim_session_scratch_for_operator` will refuse it at `src/scratch.rs:512`. The dry run misreports.

### F6 — The refusal taxonomy has no member for "cannot be proven owned"

`ReclaimRefusal` (`src/scratch.rs:562-577`) has exactly two members: `Symlink` and `NonRegular`, with stable labels `symbolic_link` and `non_regular_entry` (`:569-576`). The protocol mirrors them one-for-one: `ScratchReclaimRefusalDto` (`crates/codegg-protocol/src/scratch.rs:39-44`). There is no member for "the owner record is missing or unreadable", so such a tree cannot be reported as a refusal — it is reported as a failure.

### F7 — The operator command's promise is unmet for a record-less tree

M007's acceptance criterion 8 (`plans/implementation/managed-agent-scratch/007-scratch-retention-sweep-and-unreclaimable-tree-disposition.md:842-844`) is that `codegg daemon scratch-reclaim --tree <digest>` "with `--apply` it removes the named tree". For a record-less tree it does not:

- `reclaim_session_scratch_for_operator` (`src/scratch.rs:354-364`) calls `reclaim_owned_resource_dir` with `ReclaimStrictness::Operator` (`:362`);
- `ReclaimStrictness` branches only inside `validate_reclaim_tree` (`src/scratch.rs:539-541`) — it relaxes the **entry-content** refusals only;
- the owner-record check (`src/scratch.rs:511-529`) is therefore unchanged under `Operator` strictness, and fails on the missing record.

M007's acceptance criterion 11 (`:848-849`) — "no removal relaxes the owner-record or containment check" — and `architecture/security.md:633-635` both bind this behaviour. M008 **keeps** it for any tree that holds content.

### F8 — The error text is confirmed on the reporting host

44 warnings of the form *"scratch sweep could not reclaim an orphaned tree"* over 8 passes at the 360-minute default interval, with the failing set growing `3 → 3 → 3 → 3 → 4 → 5 → 5 → 13`. All 13 failures in one pass occurred within 9 ms. The scratch root `~/Library/Caches/codegg/scratch/sessions` held **13 tombstones and 120 live trees**; 11 of the 13 failing digests had no live tree, and 2 did — those two are the digests whose live tree is blocked by F3.

### F9 — The orphan predicate and its two producers

The orphan rule is "the digest matches no durable session" (`src/core/scratch_sweep.rs:343-346`, `:401-404`) plus a 24-hour grace. Two producers create such trees: permanent project deletion removes the session rows with **no filesystem half** (`crates/codegg-core/src/project_catalog.rs:1022-1028`), and the test suite. Both are **out of scope** for M008 (§5); M008 makes their residue *disposable*, not *absent*.

### F10 — Test hygiene is incomplete

`scratch_cache_root()`'s test redirect is `#[cfg(test)]` (`src/scratch.rs:111-114`), so it is **not compiled into the integration-test binary** and the redirect is invisible to `tests/`. `src/scratch.rs`'s own unit tests never install it either. The integration tests that *do* isolate the root set `HOME` in-process (`tests/scheduler_jobs/scratch_retention_sweep.inc`, `tests/scheduler_jobs/scratch_usage_census.inc`), but `tests/scheduler_jobs/managed_scratch_job_admission.inc` has no isolation.

### F11 — No regression evidence covers the defect class

The only owner-record test is `owner_record_recovers_from_an_interrupted_prefix` (`src/scratch.rs:1735-1746`); it exercises `ensure_owner_record` directly and never the sweep. No test drives a sweep pass over a tree whose owner record is missing, so nothing in the suite observes F1, F2, F3, F5, or F7.

### F12 — No closure, plan, or ADR covers this failure class

M007's closure names exactly one residual (`plans/closure/managed-agent-scratch/007-status.md:443-446`): *"A tree the automatic path still refuses stays quarantined until an operator disposes of it."* That residual names the **entry-content** refusal. It does not name the record-less tree, the taxonomy defect, the census disagreement, or the tombstone churn.

### F13 — One open diagnostic, explicitly not a blocker

On the reporting host, 13 tombstones were renamed within the same second on two occasions by a process that does not write to the daemon's log. Every repo code path that could do it is either daemon-only (and logs) or isolated (verified by running it and observing that it did not touch the real root). `fs_usage` requires root. This is recorded here so a successor does not have to rediscover it; it is **not** a product defect and **not** in scope (§5).

## 4. Invariants that must not regress

- **The owner-record requirement is unchanged for any tree that holds content.** `architecture/security.md:633-635` and M007 §4 (`plans/implementation/managed-agent-scratch/007-scratch-retention-sweep-and-unreclaimable-tree-disposition.md:327-330`) bind this. A tree with content and no owner record is **refused**, by the automatic path and by `--apply` alike. The only tree that may be removed without an owner record is one that holds **no entries at all**.
- **The containment check is unchanged.** Every removal resolves to a direct child of the canonical sessions root, and a symlinked or reparse-point component is refused (`src/scratch.rs:499-510`).
- **The ownership check is unchanged.** The tree's owner must match the sessions-root owner and its mode must be owner-only.
- **There is one removal routine.** Every removal goes through `reclaim_owned_resource_dir` → `validate_and_remove_tombstone` → `validate_reclaim_tree`. M007 §4 (`plans/implementation/managed-agent-scratch/007-scratch-retention-sweep-and-unreclaimable-tree-disposition.md:323-326`) calls a second removal routine a defect. M008 must reuse the existing path and the existing `ReclaimStrictness` switch.
- **Scratch stays disposable.** Nothing here makes a scratch tree durable, authoritative, or a source of results. Transcripts, durable run results, and managed Git worktrees stay outside the scratch tree.
- **The sweep performs no size traversal on its decision path.** The decision reads durable state; M007's §13.10 promise ("the sweep never blocks a tool call, a permission check, a turn admission, or a scheduler decision") is unchanged. The emptiness predicate M008 introduces is a **single directory read of the tree's own entry list**, not a recursive walk, and it runs only on a tree that has already been selected for reclaim — never on a tool call, a permission check, or a turn admission.
- **A live turn never loses its `TMPDIR`.** The lease derived from durable state is untouched.
- **The census stays read-only.** It creates, renames, and removes nothing, follows no link, reads no content, and mutates nothing.
- **The sweep's bounds stay bounded.** `SWEEP_MAX_TREES`, `SWEEP_MAX_RECLAIMS`, and `SWEEP_WALL_CLOCK_MS` are unchanged, and a truncated pass is still never presented as a complete sweep.
- **No new durable state, no new table, no migration, no `STORAGE_LAYOUT_VERSION` change.**

## 5. Scope

### In scope

- The reclaim path's **error taxonomy**: a reclaim failure is reported as a reclaim failure.
- The **empty-vs-unowned distinction** in `validate_and_remove_tombstone`: an empty tree with no owner record is reclaimed; a tree with content and no owner record is refused with a new typed reason.
- The **refusal taxonomy**: a third `ReclaimRefusal` member and its protocol mirror.
- The **sweep's accounting** for the new refusal: `refused`, not `failed`; no pass-level retry; stable tombstone identity across passes.
- The **census's verdict**: `owner_record_present` becomes gating for `reclaimable`.
- The **operator surface**: the CLI reports the new reason and never says "failed to provision managed scratch".
- **Test isolation**: the remaining unisolated scratch tests, and a deliberate, shared way for an integration test to redirect the scratch root.
- A **static guard** with a `--self-test` for the two invariants the fix depends on.

### Explicitly out of scope

- **Disposing of a content-bearing record-less tree.** That would change the ownership contract in `architecture/security.md` and M007 §4. §14 requires the implementation to stop and propose an ADR instead. It is not decided here, and this plan does not pre-authorize it.
- **The retention policy, the lease, the sweep cadence, the orphan grace period, or the sweep's bounds.** All unchanged.
- **The two orphan producers** (F9): permanent project deletion's filesystem half, and the test suite's residue. M008 makes their residue disposable; it does not stop them producing trees.
- **The entry-content refusals** (`symlink`, `non_regular_entry`). They stay, and the operator path keeps relaxing them.
- **A second removal routine, or a bypass of the owner-validated path.** A defect if introduced.
- **Windows native qualification.** The code is shared between Unix and Windows (`src/scratch.rs:491-533` is compiled under `#[cfg(any(unix, windows))]`), so the empty-tree rule is implemented once and applies to both. Windows *native containment* remains unqualified and stays tracked in `plans/subsystems/managed-agent-scratch-native-qualification-corrective-addendum.md`.
- **The F13 diagnostic** (which process renamed the reporting host's tombstones). It is a host observation, not a product change.
- **The desktop application.** It does not consume the scratch DTOs; `npm run bindings:check` must confirm that stays true.

## 6. Required production changes

### Core/domain

- **Split `ScratchError::Io`.** `#[from] std::io::Error` may exist on only one variant, so the reclaim path must stop relying on it: every I/O error inside `reclaim_owned_resource_dir`, `recover_reclaim_tombstones`, and `validate_and_remove_tombstone` is mapped **explicitly** to a reclaim-named variant whose display text names reclamation. The allocation path keeps the provisioning variant.
- **Make the owner-record check explicit instead of `?`-propagating.** A missing owner record must reach a typed decision, not an `io::Error`:
  - the record is **missing** → the tree is either empty (reclaim it) or holds content (refuse it with `ownership_unverifiable`);
  - the record is present but **malformed** → `ScratchError::InvalidPath`, unchanged from today;
  - the record is present but **owned by another user** or is not owner-only → `ScratchError::WrongOwner`, unchanged from today.
- **Add the third refusal member.** `ReclaimRefusal` gains `OwnershipUnverifiable` with the stable label `ownership_unverifiable`, and the label is what the operator surface prints. The census's refusal reason uses the same member, so the census and the validator name a reason identically.
- **The empty-tree predicate.** A tree may be removed without an owner record **only** when a single read of its own entry list yields **zero** entries. The predicate must also confirm the tree is a real directory (not a symlink or a reparse point), a direct child of the canonical sessions root, owned by the sessions-root owner, and owner-only. A read that fails is treated as "not empty" — fail closed. The predicate must not follow a link and must not recurse.

### Storage and migrations

- None. No table, no column, no migration, no `STORAGE_LAYOUT_VERSION` change. The refusal is a **typed in-memory outcome**, and the sweep's counters are pass-local.

### Protocol and DTOs

- `ScratchReclaimRefusalDto` gains `OwnershipUnverifiable` (`crates/codegg-protocol/src/scratch.rs:39-44`), and the mapping in `src/core/daemon_scratch.rs:300-304` and the label in `src/main.rs:876-885` are extended to match.
- `SCRATCH_PROTOCOL_VERSION` (`crates/codegg-protocol/src/scratch.rs:29`) is bumped to 3 and the module's compatibility section records the change, following the module's own precedent for version 2. The surface is a local daemon socket plus the CLI; the desktop does not consume it.
- `ScratchTreeUsageDto.owner_record_present` keeps its meaning (a fact about the tree) and stays additive. What changes is the **`reclaimable` verdict** that is computed alongside it.

### Runtime and concurrency

- The sweep's **orphan** and **session** arms classify the new refusal as `refused` — the counter already exists — and never as `failed`. A refusal therefore arms **no** pass-level retry (`src/core/scratch_sweep.rs:762-770`).
- The **tombstone's identity is stable across passes**: two consecutive passes that both refuse the same digest leave the sessions root with the same set of tombstone names for that digest. The claim rename (`src/scratch.rs:472-473`) is the mutual-exclusion mechanism between concurrent reclaimers and is retained; the requirement is on the **observable outcome** of a pass that ends in a refusal, not on the mechanism. Restoring the pre-pass name on a refusal, or validating the tombstone in place, are both acceptable implementations; the acceptance criterion in §13.3 is what decides.
- A digest is attempted **at most once per pass** across the session and orphan arms, so a tree with both a live tree and a tombstone is not reported twice in one pass (F3's observed symptom).
- The reclaim path keeps its process-global mutual exclusion (`SHARED_RECLAIM_LOCK`, `src/scratch.rs:336-339`).

### Frontend or operator surface

- `codegg daemon scratch-reclaim --tree <digest> --apply` against a tree with content and no owner record: reports the reason `ownership_unverifiable`, removes **nothing**, and returns the refusal verdict rather than a failure.
- `codegg daemon scratch-reclaim --tree <digest> --apply` against an **empty** tree with no owner record: removes it and reports `removed`.
- `codegg daemon scratch-usage`: a tree with content and no owner record is reported as **not reclaimable** with the reason `ownership_unverifiable`; a tree with no owner record and no entries is reported as reclaimable. The tri-state "unknown" for a truncated walk is unchanged.
- No operator-facing text says "failed to provision managed scratch" for a reclaim failure.

### Security and authorization

- The empty-tree removal is authorised by exactly four checks, all of which the existing path already performs: the resolved path is a direct child of the canonical sessions root; the entry is a real directory and not a symlink or reparse point; the owner matches the sessions-root owner and the mode is owner-only; the tree holds **zero** entries.
- A tree that fails any of the first three checks is refused exactly as it is today, whatever its entry count.
- The refusal removes nothing and grants nothing. It is a **typed, distinguishable, non-destructive** outcome.
- The operator path is unchanged in its authority: it still relaxes the **entry-content** refusals only, it still honours the lease, it is still audited as `scratch_reclaim` with bounded metadata, and it is still served on the local daemon socket only.

### Documentation and static guards

- `architecture/security.md` §"Operator disposition (M007)" gains the new refusal reason and the empty-tree rule, and keeps the existing sentence that the owner-record check is unchanged for a tree the daemon still owns.
- `architecture/process-tool-execution-ownership.md` gains one sentence on the record-less case.
- A new static guard, `scripts/check_scratch_reclaim_invariants.py`, asserting the two invariants the fix depends on:
  1. **no reclaim-path function propagates a raw I/O error into the provisioning variant** — the reclaim functions must map their I/O failures explicitly;
  2. **no code path removes a tree without an owner record except under an explicit emptiness guard** — the only removal that may skip the owner record must be guarded by a zero-entries predicate.
  It carries a `--self-test` that injects each violation and proves the guard rejects it, and it is wired into **both** `scripts/verify.sh quick` and `.github/workflows/ci.yml`, per `AGENTS.md` §"Automatic guards".

## 7. Ordered work packages

### Work package A — Split the reclaim error taxonomy

Intent: a reclaim failure must never be reported as a provisioning failure.

Required changes:

- Split `ScratchError::Io` into a provisioning variant (unchanged text, used by the allocation path) and a reclaim variant whose text names reclamation.
- Replace every `?` on an `io::Result` inside `reclaim_owned_resource_dir`, `recover_reclaim_tombstones`, and `validate_and_remove_tombstone` with an explicit mapping to the reclaim variant.
- Confirm no other `ScratchError` variant's text names provisioning on a path that is not provisioning.

Acceptance evidence:

- A unit test that forces a reclaim validation failure and asserts the rendered error contains a reclamation phrase and **not** the substring `failed to provision managed scratch`.
- `python3 scripts/check_scratch_reclaim_invariants.py --self-test` proves the guard rejects an injected reclaim-path `?`-propagation.

### Work package B — Distinguish "already gone" from "cannot be owned"

Intent: an empty record-less tree is reclaimed; a content-bearing record-less tree is refused with a typed reason.

Required changes:

- In `validate_and_remove_tombstone`, replace the `?` on `symlink_metadata(&owner_record)` with an explicit match on `ErrorKind::NotFound` and an explicit read of the tree's entry list.
- On a **missing** record with **zero** entries: treat the tree as already reclaimed and remove it, after the containment, real-directory, and ownership checks.
- On a **missing** record with **one or more** entries: return `ScratchError::Unreclaimable { reason: ReclaimRefusal::OwnershipUnverifiable, path }`.
- A **malformed** record stays `ScratchError::InvalidPath`; a **foreign-owned** record stays `ScratchError::WrongOwner`. Neither is removed by any path.
- Add `ReclaimRefusal::OwnershipUnverifiable` and its protocol mirror, and extend the two label maps (`src/core/daemon_scratch.rs:300-304`, `src/main.rs:876-885`).
- `ReclaimStrictness::Operator` must **not** change any of this: the operator path refuses a content-bearing record-less tree too, and the plan states this explicitly in §13.9.

Acceptance evidence:

- Four unit tests: empty + no record → removed; content + no record → refused with `ownership_unverifiable`; malformed record → `InvalidPath`; foreign-owned record → `WrongOwner`.
- A test that the **operator** path removes an empty record-less tree and refuses a content-bearing one.

### Work package C — Bounded refusal accounting and stable tombstone identity

Intent: a permanent refusal must not be counted as a failure, must not arm the pass-level retry, and must not churn the tombstone's name.

Required changes:

- The sweep's orphan arm (`src/core/scratch_sweep.rs:421-437`) and session arm (`:374-382`) classify the new refusal as `refused`.
- A pass that only encounters refusures reports `failed == 0`, so the 15-second retry (`:762-770`) is never armed.
- A digest is attempted at most once per pass, so a tree with both a live tree and a tombstone is reported once, not twice.
- The tombstone's name is stable across passes that end in a refusal.

Acceptance evidence:

- A sweep test that runs **two consecutive passes** over a permanently-refused tree and asserts, for each pass, `failed == 0` and `refused == 1`, and asserts that the sessions root's set of tombstone names for that digest is identical after the two passes.

### Work package D — Make the census agree with the validator

Intent: `scratch-usage` must never report a tree as reclaimable that the reclaim path will refuse.

Required changes:

- The census's `reclaimable` verdict becomes **gating** on the owner-record rule: a tree with no owner record and at least one entry is not reclaimable, and its refusal reason is `ownership_unverifiable`.
- A tree with no owner record and no entries is reclaimable — the automatic path *would* remove it — so the census and the validator agree in **both** directions.
- The tri-state "unknown" for a truncated or vanished walk is unchanged.

Acceptance evidence:

- A census test over a content-bearing record-less tree asserts `reclaimable == Some(false)` and `refusal == Some(OwnershipUnverifiable)`.
- A census test over an empty record-less tree asserts `reclaimable == Some(true)`.
- A test that the census's verdict and the reclaim path's actual outcome agree for both trees — the census is not a separate opinion.

### Work package E — Isolate the scratch tests

Intent: no scratch test may read or write the developer's real scratch root.

Required changes:

- Provide a deliberate, shared way for an **integration** test to redirect the scratch root. The `#[cfg(test)]` redirect (`src/scratch.rs:111-114`) is not compiled into the integration-test binary, so the current pattern — setting `HOME` in-process, as `tests/scheduler_jobs/scratch_retention_sweep.inc` and `tests/scheduler_jobs/scratch_usage_census.inc` already do — is the baseline the repository should use. The implementer may instead expose the redirect deliberately to integration tests; that is a design choice, but the outcome is fixed.
- Bring `src/scratch.rs`'s own unit tests under isolation.
- Bring `tests/scheduler_jobs/managed_scratch_job_admission.inc` under isolation.

Acceptance evidence:

- A test that proves the redirect is effective: under a redirected root, the real scratch root's manifest is byte-identical before and after the test.
- The suite runs green on macOS, where `/tmp` and `/var` are symlinks the allocator deliberately refuses.

### Work package F — Ship the static guard

Intent: the two static invariants the fix depends on are enforced, not merely asserted in prose.

Required changes:

- `scripts/check_scratch_reclaim_invariants.py` with the two checks in §6 and a `--self-test` that injects each violation and proves the guard rejects it.
- Wire it into `scripts/verify.sh quick` **and** `.github/workflows/ci.yml`.
- Follow the repository convention that a guard is wired automatically when its defect would otherwise fail silently (`AGENTS.md` §"Automatic guards").

Acceptance evidence:

- `python3 scripts/check_scratch_reclaim_invariants.py --self-test` output showing both injected violations rejected.
- `python3 scripts/check_scratch_reclaim_invariants.py` green on the closure head.

### Ordering

A → B → C → D → E → F. B depends on A's taxonomy (the refusal travels as a typed error). C and D both depend on B's refusal member. E is independent of A–D and can run in parallel. F is written last so its `--self-test` targets the final shape of the code.

## 8. Failure, cancellation, restart, and contention semantics

- **Partial failure during removal.** A daemon killed between `remove_dir_all`'s child removal and the directory removal leaves an empty tombstone. After this milestone the reclaim path is **resumable**: the next pass removes it, because an empty tree with no owner record is already reclaimed. This is the specific class F4 identifies.
- **A daemon killed mid-`ensure_owner_record`.** The allocation path's existing interrupted-prefix recovery (`src/scratch.rs:831`, `:839-841`) is unchanged and remains the repair for that case. M008 does not weaken it.
- **Concurrent reclaimers.** The claim rename (`src/scratch.rs:472-473`) remains the mutual-exclusion mechanism: only the rename winner proceeds, and the loser sees `NotFound` and skips. A refusal that ends a pass must leave the tombstone in a state a later pass can re-claim — the stability requirement in §13.3 is about the **name set**, and the implementer must ensure a refusal does not make the tombstone un-reclaimable (a tombstone stranded under a name the next pass will not look for is a defect).
- **A tree recreated at the live name.** Unchanged. The tombstone path never touches the live name, and the live tree's reclaim (`src/scratch.rs:430-433`) happens only after the tombstone recovery has succeeded.
- **Duplicate operator delivery.** `codegg daemon scratch-reclaim --tree <digest> --apply` is idempotent: a second invocation on an already-removed tree reports `absent` and removes nothing.
- **Restart.** A daemon restart re-runs the sweep. A previously-refused tree is refused again, counted as `refused`, never as `failed`, and arms no retry.
- **Cancellation.** A cancelled pass stops at the next bound and records the bound, exactly as today (`src/core/scratch_sweep.rs:357-364`).
- **Stale generations.** The tombstone's uuid is the generation marker. A refusal must not lose it.

## 9. Compatibility and migration

- **Storage.** No migration, no durable state, no `STORAGE_LAYOUT_VERSION` change, no new configuration key.
- **Protocol.** `ScratchReclaimRefusalDto` gains a variant. The scratch protocol surface is a local daemon socket plus the CLI; the desktop application does not consume it (`npm run bindings:check` must confirm this). `SCRATCH_PROTOCOL_VERSION` is bumped to 3 with the module's compatibility section updated, following the module's own precedent for version 2 (`crates/codegg-protocol/src/scratch.rs:22-29`).
- **Behaviour change an operator will notice.** A sweep pass that previously logged *"scratch sweep could not reclaim an orphaned tree"* with the provisioning text and incremented the failure count now records a **refusal** with the reason `ownership_unverifiable`. This is recorded in `architecture/security.md` and in the closure record, because a silently quieter log is a change operators watch for.
- **Legacy paths.** None are removed. The single removal routine, the tombstone path, the owner-record check for content-bearing trees, the census, and the operator command all remain.
- **Removal criteria.** Not applicable — nothing is deprecated.

## 10. Required tests

### Focused unit tests

- `validate_and_remove_tombstone`: empty + no record → removed; content + no record → `Unreclaimable { reason: OwnershipUnverifiable }`; malformed record → `InvalidPath`; foreign-owned record → `WrongOwner`; symlinked tree → `InvalidPath`; tree outside the canonical sessions root → `InvalidPath`.
- The reclaim error text: a reclaim failure's rendered message names reclamation and does not contain `failed to provision managed scratch`.
- The empty-tree predicate fails closed: a directory whose entry list cannot be read is treated as non-empty.

### Integration tests

- The `scheduler_jobs` family (`tests/scheduler_jobs/mod.rs`) gains a member that drives a **full sweep pass** over a tree whose owner record is missing, asserting the end state of the sessions root.
- The census's verdict and the reclaim path's actual outcome agree for a content-bearing record-less tree and for an empty one.

### Restart and recovery tests

- A sweep pass over an **empty** record-less tree removes it and the pass reports no failure — the resumed-removal case (F4).
- A daemon restart between a tombstone creation and its removal leaves the next pass able to reclaim or refuse it, and never to fail on it.

### Contention and cancellation tests

- Two concurrent sweep passes over the same refused tree: the tombstone name set is unchanged, and the digest is attempted at most once per pass.
- A cancelled pass stops at its bound and records the bound.

### Security and negative tests

- The **operator** path refuses a content-bearing record-less tree and removes nothing (M007 §13.11 preserved).
- The **automatic** path refuses a content-bearing record-less tree and removes nothing.
- A tree whose owner record names a different digest is refused.
- A tree whose owner record is owned by another user is refused.
- A symlinked tree is refused and the link's target is untouched.

### Migration and compatibility tests

- None are required — there is no migration. The protocol's new refusal variant is covered by the existing protocol serialization tests plus one case for the new member.

## 11. Required verification commands

```bash
# narrow tests first
cargo test -p codegg --lib scratch --locked
cargo test --test scheduler_jobs -- scratch_retention_sweep --locked
cargo test --test scheduler_jobs -- scratch_usage_census --locked
cargo test --test scheduler_jobs -- managed_scratch_job_admission --locked
cargo test -p codegg-protocol --locked

# static guards
python3 scripts/check_scratch_reclaim_invariants.py --self-test
python3 scripts/check_scratch_reclaim_invariants.py

# formatting and linting
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings

# broader suite appropriate to the change
bash scripts/verify.sh quick
```

Do not claim commands that were not actually run in the closure record.

## 12. Documentation updates

- `architecture/security.md` §"Operator disposition (M007)" (`:626-640`) — the new refusal reason, the empty-tree rule, and the note that a refusal no longer appears as a provisioning failure. The owner-record and containment sentences are **kept**, not weakened.
- `architecture/process-tool-execution-ownership.md:35-49` — one sentence on the record-less case.
- `crates/codegg-protocol/src/scratch.rs` — the compatibility section and `SCRATCH_PROTOCOL_VERSION`.
- `plans/subsystems/managed-agent-scratch-roadmap.md` — §7 gains M008; §12 gains the M008 row.
- `plans/registry.md` — the "Managed agent scratch spaces" roadmap row and a new row under "Dependency-ready implementation plans".
- `plans/closure/managed-agent-scratch/008-status.md` — the closure record, when the milestone is closed.

## 13. Acceptance criteria

These are externally observable and are what the closure record must be able to point at.

1. A sweep pass over a scratch tree whose owner record is missing and whose directory holds **no entries** completes that tree's reclaim: the tree is gone after the pass, and the pass reports **no** failure.
2. A sweep pass over a scratch tree whose owner record is missing and whose directory holds **at least one entry** completes the pass with that tree counted as **refused**: the pass reports **zero** failures, the 15-second pass-level retry is **not** armed, and the tree remains on disk.
3. Two consecutive sweep passes over the same refused tree leave the sessions root containing the **same set of tombstone names** for that digest.
4. `codegg daemon scratch-usage` reports a tree with no owner record and at least one entry as **not reclaimable**, with the reason `ownership_unverifiable`.
5. `codegg daemon scratch-usage` reports a tree with no owner record and no entries as reclaimable.
6. `codegg daemon scratch-reclaim --tree <digest> --apply` against a tree with content and no owner record removes **nothing** and reports the reason `ownership_unverifiable`.
7. `codegg daemon scratch-reclaim --tree <digest> --apply` against an **empty** tree with no owner record removes it and reports `removed`.
8. No error surfaced by a reclaim failure contains the text `failed to provision managed scratch`.
9. The owner-record requirement is unchanged for every tree that holds **content**: a malformed owner record is refused with `InvalidPath`, a foreign-owned owner record is refused with `WrongOwner`, and neither is removed by the automatic path or by `--apply`.
10. `scripts/check_scratch_reclaim_invariants.py --self-test` proves the guard rejects an injected violation of each of its two checks, and the guard runs in **both** `scripts/verify.sh quick` and `.github/workflows/ci.yml`.
11. `scripts/verify.sh quick`, the focused suites, and the hosted CI run on the pull-request head are green.
12. The registry and the roadmap name M008 with the same status the closure records, and the closure record states plainly that a **content-bearing** record-less tree is still refused.

## 14. Stop conditions

The implementing agent must **stop and report** rather than improvise if:

- **Disposing of a content-bearing record-less tree turns out to be necessary.** That change alters the ownership contract in `architecture/security.md` and M007 §4, so it requires an ADR. Propose the ADR; do not implement the change.
- The empty-tree rule cannot be shown safe — for instance if the emptiness predicate would have to follow a link, recurse, or accept a tree whose entry list could not be read.
- The owner-record check, the containment check, or the ownership check would have to be weakened for any tree that holds content.
- A second removal routine would be needed, or the existing owner-validated path would have to be bypassed.
- The census would have to stop being read-only, or the sweep's decision path would have to gain a size traversal.
- The protocol crate cannot carry the new refusal variant without breaking a pinned consumer; report the consumer and the breakage rather than widening the change.
- The existing single-reclaim-mutex design cannot express the required concurrency semantics, and the alternative would change the reclaim path's authority.
- A hard dependency named in §2 turns out not to be closed at the implementation head.
- Repository evidence contradicts a canonical invariant in §4, or the baseline has moved such that the anchors in §3 no longer describe the code — re-measure and report the difference rather than proceeding from stale anchors.
- Scope would expand into another subsystem roadmap (the two orphan producers in F9, or the F13 diagnostic).

## 15. Closure evidence required

The later closure record `plans/closure/managed-agent-scratch/008-status.md` must contain:

1. The exact commit SHA of the implementation head, and the exact-head output of every command in §11.
2. The hosted CI run identifier on the pull-request head, and the run identifier on the merged head.
3. The guard's `--self-test` output, showing both injected violations rejected, and the green run of the guard itself.
4. The four `validate_and_remove_tombstone` unit-test results, the sweep's two-pass test result, and the census-agreement test results, named by test id.
5. The operator-surface evidence for §13.6 and §13.7 — the actual command output.
6. A **before/after observation** on a host that has the residue: the count of record-less tombstones and the sweep's refusal count before and after. If no such host is available, the record must say so plainly and name the synthetic evidence that stands in for it.
7. An explicit statement of what was **not** done: content-bearing record-less trees are still refused; the two orphan producers still produce trees; Windows native containment is still unqualified.
8. The registry and roadmap reconciliation, with the exact rows changed.
9. The F13 diagnostic's status: still open, or closed by a named observation. It is not a blocker and its closure is not required for this milestone.

## 16. Handoff notes

- **The reclaim path is one routine.** `reclaim_owned_resource_dir` → `validate_and_remove_tombstone` → `validate_reclaim_tree`. Reuse it; do not add a second removal routine.
- **The scratch root is `dirs::cache_dir()/codegg/scratch/sessions`.** Every test must redirect it. The `#[cfg(test)]` redirect (`src/scratch.rs:111-114`) is **not** compiled into the integration-test binary; the repository's baseline pattern is to set `HOME` in-process, as `tests/scheduler_jobs/scratch_retention_sweep.inc` does. Work package E exists because three call sites still do not.
- **macOS `/tmp` and `/var` are symlinks**, and the allocator deliberately refuses a symlinked path component. A test that builds a temp root under `/var/folders` may fail with `InvalidPath` at the allocator's symlink refusal. Use the repository's existing isolation pattern rather than inventing one.
- **The reclaim path holds a process-global mutex** (`SHARED_RECLAIM_LOCK`, `src/scratch.rs:336-339`). Do not call it concurrently from within one process.
- **The refusal member must be mirrored in four places**: `src/scratch.rs` (the enum), `crates/codegg-protocol/src/scratch.rs` (the DTO), `src/core/daemon_scratch.rs:300-304` and `src/main.rs:876-885` (the two label maps).
- **The census is read-only.** No work package may write during a census, and the census's verdict must be *derived* from the reclaim validator's rule, not restated independently — M006's own commitment (`architecture/security.md`, the census paragraph) is that "a report cannot claim a tree is reclaimable when the validator would refuse it".
- **Windows.** `src/scratch.rs:491-533` is compiled under `#[cfg(any(unix, windows))]`, so the empty-tree rule is implemented once. The `#[cfg(not(any(unix, windows)))]` fallbacks return `CleanupUnavailable` and must keep compiling.
- **Preserve the developer's environment.** Do not run a test, a census, or a sweep against the real scratch root; the reporting host's residue is evidence, not a test fixture.
- **The reporting host's live daemon is gone** (`core.sock` and `core.pid` are absent, and the log's last line is the 15:28:01 sweep). A fresh observation of the residue must start a daemon deliberately, and the closure must say that it did.
