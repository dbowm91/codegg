# Development Verification and Release Final Evidence Closure Addendum

Status: active — M008 corrective hosted-verification closure in progress

Parent roadmap:

- `plans/subsystems/development-verification-release-roadmap.md`

Predecessor corrective addendum:

- `plans/subsystems/development-verification-release-correctness-closure-addendum.md`

Predecessor implementation:

- `plans/implementation/development-verification-release/005-green-verification-and-crates-io-closure.md`

Historical final corrective implementation:

- `plans/implementation/development-verification-release/006-final-evidence-and-release-documentation-closure.md`

Current final implementation and closure:

- `plans/implementation/development-verification-release/007-minimal-verification-contract-and-final-closure.md`

Historical target closure record:

- `plans/closure/development-verification-release/006-status.md`

Current target independent closure record:

- `plans/closure/development-verification-release/007-status.md`

## 1. Purpose

M006 is historical implementation evidence. M007 is the controlling final
verification contract and supersedes M006's breadth and release-evidence
requirements without rewriting its historical record.

M005 landed the substantive verification and manual-release corrections, but independent review found that strict subsystem closure is still unsupported by final-head evidence and active documentation.

This addendum transferred final closure ownership to M007. It does not reopen the broad CI simplification or authorize additional runtime cleanup.

The governing rule is:

> Preserve the reduced one-job, local-first architecture; correct the remaining guard and release-document defects; then prove one exact revision locally and in GitHub Actions before closing the subsystem.

## 2. Accepted M005 implementation

The following M005 outcomes are retained:

- one routine, read-only GitHub Actions `verify` job;
- no automated release workflow, registry credentials, release artifacts, schedules, audit lane, LSP evidence matrix, or cross-target hot-path build;
- baseline-aware Tokio flavor enforcement;
- aligned local/hosted broad-test resource settings;
- scheduler-owned Bash/Python routing correction;
- publishable Cargo metadata and path-plus-version internal dependencies;
- manual crates.io publication as the intended release path.

M006 must not reverse these outcomes.

## 3. Findings transferred to M006

### F06 — Final-head local and hosted evidence is absent

The repository advanced through substantive scheduler, process-cleanup, Tool Programs, LSP, and test-fixture commits after the first M005 verification summaries. Earlier results do not prove the final accepted revision.

Strict closure requires:

- `scripts/verify.sh quick` on the final M006 implementation SHA;
- `scripts/verify.sh full` on the same SHA;
- one successful hosted `verify` job attached to that SHA.

### F07 — Package inventory evidence is stale

The checked-in M005 package inventory contains internal dependency relationships that do not match current manifests. It also contains contradictory verification statements and references an earlier implementation state.

M006 must regenerate the inventory from current Cargo metadata and manifests.

### F08 — Manual release instructions are operationally incorrect

`RELEASING.md` currently conflates first-publication name availability with ownership of an existing crate and checks index propagation for packages before those packages are published.

M006 must separate authentication, initial name checks, and existing-crate ownership, and must query packages actually published in the preceding step.

### F09 — Tokio guard closure contract is incomplete

The guard still excludes the entire `examples` directory, allows unresolved attribute/function associations to become baseline identities, and lacks focused production-path tests for all required comparison semantics.

M006 must make the guard fail closed and remove broad repository-owned source exclusions.

### F10 — Planning state is ambiguous

The predecessor addendum and registry disagree on M005 status, and no single final closure owner is registered.

M006 becomes the sole dependency-ready final closure plan. M005 is retained as implemented but conditionally accepted; no `005-status.md` is required or created as part of this transfer.

## 4. M006 ownership boundary

M006 owns:

- narrow Tokio guard/test corrections;
- current-head local and hosted evidence;
- package inventory regeneration;
- `RELEASING.md` command and explanation corrections;
- active planning reconciliation;
- independent strict closure.

M006 does not own:

- additional production/runtime/test-suite cleanup unrelated to the guard or documentation;
- CI expansion;
- actual crates.io publication;
- crate renaming or version selection;
- a local registry service or release framework;
- historical-record rewriting.

If current-head verification reveals an unrelated runtime/product failure, M006 must stop and register a separate corrective plan rather than absorb it.

## 5. Dependency graph

```text
M001–M004 structural simplification
          |
          v
M005 verification/release implementation
          |
          v
Independent post-M005 review (F06–F10)
          |
          v
M006 final evidence/documentation correction
          |
          v
Independent M006 strict closure
```

M006 is dependency-ready against baseline `db890ac138fe18c6bae3de991b70dc007789c8a0`.

## 6. Strict closure requirements

The subsystem may be closed only when all of the following are true on one accepted revision:

1. the routine workflow remains one read-only, non-release `verify` job;
2. the Tokio guard scans repository-owned example source and rejects malformed/unresolved bare attributes;
3. focused guard tests prove historical-pass, new-failure, stale-failure, malformed-failure, and deterministic-baseline behavior;
4. `scripts/verify.sh quick` exits zero;
5. `scripts/verify.sh full` exits zero;
6. a successful GitHub Actions `verify` run is recorded for the same SHA;
7. the package inventory matches current manifests and contains no contradictory command results;
8. leaf package checks pass and dependent unpublished-registry sequencing results are honestly classified;
9. `RELEASING.md` correctly distinguishes initial and subsequent release preflight and checks propagation of packages already published;
10. no automated publication authority or broader CI machinery is introduced;
11. a separate reviewer creates `plans/closure/development-verification-release/006-status.md` and finds no unresolved high or medium issue.

A missing hosted run, nonzero canonical verification command, registry ownership conflict, or unrelated current-head runtime failure is a blocker, not a pass.

## 7. Milestone status

| Milestone | Status | Implementation plan | Closure record | Disposition |
|---|---|---|---|---|
| 001 — Routine CI contraction | conditionally closed | `plans/implementation/development-verification-release/001-routine-ci-contraction.md` | `plans/closure/development-verification-release/001-status.md` | Historical structural implementation retained |
| 002 — Canonical local verification contract | conditionally closed | `plans/implementation/development-verification-release/002-local-verification-contract.md` | `plans/closure/development-verification-release/002-status.md` | Historical implementation retained |
| 003 — Manual crates.io release ownership | conditionally closed | `plans/implementation/development-verification-release/003-manual-crates-io-release-ownership.md` | `plans/closure/development-verification-release/003-status.md` | Historical automated-release removal retained |
| 004 — Optional integration evidence cleanup and closure | conditionally closed | `plans/implementation/development-verification-release/004-integration-evidence-cleanup-and-closure.md` | `plans/closure/development-verification-release/004-status.md` | Historical structural cleanup retained |
| 005 — Green verification and crates.io correctness implementation | conditionally closed | `plans/implementation/development-verification-release/005-green-verification-and-crates-io-closure.md` | — | Substantive implementation retained; strict closure transferred to M006 |
| 006 — Final evidence and release documentation closure | conditionally closed | `plans/implementation/development-verification-release/006-final-evidence-and-release-documentation-closure.md` | `plans/closure/development-verification-release/006-stop-condition.md` | Historical implementation and stop-condition evidence retained; final verification ownership transferred to M007. |
| 007 — Minimal verification contract and final closure | closed | `plans/implementation/development-verification-release/007-minimal-verification-contract-and-final-closure.md` | `plans/closure/development-verification-release/007-status.md` | Boundary guard corrected; focused, quick, and shared hosted evidence passed; Provider M007, Tool Programs M019, and Agent Runtime M017 are strictly closed. |
| 008 — Hosted Clippy Review module ordering | active | `plans/implementation/development-verification-release/008-hosted-clippy-review-module-ordering.md` | — | Separate corrective follow-up for the pre-existing `items_after_test_module` finding exposed after M005 Clippy corrections. |
| 009 — Built-in agent test contract alignment | closed | `plans/implementation/development-verification-release/009-builtin-agent-test-contract-alignment.md` | `plans/closure/development-verification-release/009-status.md` | Stale nine-agent expectations were aligned to the checked-in ten-agent assets; no agent assets or runtime behavior changed. |
