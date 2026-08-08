# Post-Audit Correctness, Simplification, and Footprint — Corrective Closure Addendum

Status: active corrective closure

Source roadmap:

- `plans/subsystems/post-audit-correctness-simplification-roadmap.md`

Source closure:

- `plans/closure/post-audit-correctness-simplification/008-status.md`

Repository/PR state reviewed:

- `main`: `8bcc15e0663d610a132bc16c2f35fe637421a1b1`
- implementation PR: #73 (`planning/post-audit-correctness-simplification` -> `main`)
- reviewed PR head: `a3c22d129f7b0c2fe462e435acfe77daa39ab48f`
- PR state at review: open, mergeable, draft

## 1. Why this addendum exists

M001-M008 production work is substantially complete and individually closed, but the
workstream was marked strictly closed before its integration state matched that claim.
PR #73 still contains the implementation outside `main`, its title/body describe only
the original M003 TUI slice, and the latest documentation-only head may have a newer
hosted CI run than the production head cited by M008.

This is a planning/integration discrepancy, not evidence that M001-M008 should be
reimplemented. The corrective pass therefore owns only the narrow work needed to make
repository state, PR state, dependency disposition, and closure records agree.

The corrective implementation plan is:

- `plans/implementation/post-audit-correctness-simplification/009-corrective-pr-integration-and-advisory-cleanup.md`

The target corrective closure record is:

- `plans/closure/post-audit-correctness-simplification/009-corrective-status.md`

This corrective pass is designated **C001**. File number `009` preserves sequential
planning filenames; it is not a new product milestone M009.

## 2. Scope

C001 owns:

- making PR #73 title/body accurately describe the integrated M001-M008 workstream;
- reconciling draft/ready-for-review state with actual merge readiness;
- confirming the latest relevant PR head has a successful existing `verify` run;
- merging the completed work to `main` when CI and repository state permit;
- confirming the merge result contains the accepted M001-M008 production tree;
- reconciling registry, roadmap/addendum, and closure records to the actual merged SHA;
- explicitly disposing the pre-existing transitive `lru` advisory noted by M008 without
  forcing a broad dependency migration into this workstream.

C001 does not own new product features or a second correctness audit of already accepted
M001-M008 code.

## 3. Invariants

- The accepted M001-M008 production implementation must not be rewritten merely to make
  the final PR easier to merge.
- Single-daemon authority, single-binary topology, scheduler ownership, protocol/storage
  compatibility, supported features, manual release cadence, and one-job routine CI remain
  unchanged.
- A documentation-only finalization commit must not invalidate an already accepted
  production-head verification claim; however, the actual PR head must still satisfy the
  repository's normal required checks before merge.
- No new CI lane, matrix, release workflow, dependency bot, audit gate, artifact bundle,
  or binary-size threshold may be introduced.
- The `lru` advisory must be classified from the actual locked dependency path before any
  dependency change is attempted.
- A transitive advisory may be deferred when the safe fix requires a broad upstream
  migration disproportionate to this closure pass, but the reason and owner must be
  recorded explicitly.
- The workstream may return to strict `closed` only after the implementation is present on
  `main` and the closure record names that merged revision.

## 4. Dependency and execution order

```text
M001-M008 closed implementation
        |
        v
C001 final PR integration and advisory disposition
        |
        v
strict merged closure on main
```

C001 has no product-code predecessor beyond the already closed M001-M008 implementation.
Its operational dependencies are:

1. PR #73 remains mergeable against current `main` or is reconciled without losing accepted
   implementation;
2. the normal existing hosted `verify` job succeeds on the merge candidate;
3. any newly observed failure is classified before changes are made.

The independent supported-Linux Landlock evidence condition in the runtime-safety roadmap
remains unrelated and does not block C001.

## 5. Advisory disposition rule

M008 records a pre-existing `lru` advisory reachable through the locked dependency graph.
C001 must inspect the actual path and current advisory metadata rather than blindly changing
versions.

Acceptable dispositions are:

1. **narrow fix** — a compatible lockfile/manifest adjustment reaches a patched version
   without broad Ratatui/TUI migration, MSRV increase, or feature change; land it with
   focused TUI/build verification;
2. **not applicable to supported use** — evidence demonstrates the vulnerable API/path is
   not reachable in the supported CodeGG configuration; record the evidence and defer
   upstream cleanup;
3. **deferred upstream migration** — the patched line requires a material Ratatui or other
   dependency migration. Record the dependency path and create no broad migration inside
   C001. A future dependency-maintenance plan may be registered only if the risk warrants
   active work.

The advisory alone must not turn C001 into a general dependency-upgrade campaign.

## 6. Exit conditions

This addendum is closed only when:

- PR #73 metadata describes the complete M001-M008 implementation;
- PR #73 is no longer draft when it is ready to merge;
- the latest merge candidate has a successful normal `verify` result;
- no unresolved review thread or concrete CI failure remains;
- the accepted implementation is merged to `main` without scope-expanding production
  changes;
- the resulting `main` SHA is recorded in the corrective closure record;
- the `lru` advisory has one explicit disposition under section 5;
- `plans/registry.md` no longer lists C001 as ready/active;
- the post-audit workstream is again recorded as strictly `closed` against the merged SHA;
- no additional corrective milestone is created solely for evidence or PR bookkeeping.

## 7. Stop conditions

Stop and report rather than broadening C001 if:

- PR #73 cannot merge without resolving a substantive conflict in M001-M008 production
  behavior;
- hosted CI exposes a new critical/high correctness or security failure;
- the advisory fix requires a major Ratatui/TUI migration, MSRV increase, or supported
  feature removal;
- `main` has independently changed a touched ownership boundary after the reviewed base;
- completing the merge would require changing protocol, storage, daemon ownership, binary
  topology, or release policy.

A concrete production defect discovered here gets its own narrowly scoped corrective plan.
Do not hide it inside PR-cleanup work.
