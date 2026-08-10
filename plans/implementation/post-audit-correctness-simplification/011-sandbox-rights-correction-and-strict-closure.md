# Post-Audit Correctness, Simplification, and Footprint C002 — Sandbox Path-Kind Rights Correction and Strict Closure

Status: implemented
Repository baseline: `d0b62204a0740195c53face071635d44c147f12b`

Source roadmap/addendum: `plans/subsystems/post-audit-correctness-simplification-corrective-closure-addendum.md`
Related corrective closure: `plans/closure/post-audit-correctness-simplification/009-corrective-status.md`
Supersedes registration stub: `plans/implementation/post-audit-correctness-simplification/010-sandbox-file-rights-correction.md`
Primary class: invariant / security correctness

## 1. Objective

Correct the narrow Landlock path-rights defect exposed by hosted verification after PR #73 merged, then complete strict closure of the post-audit workstream if the normal hosted gate is green.

The defect is specific: `/dev/null` is a character device, but `src/security/sandbox.rs::add_landlock_path_rule` currently removes the directory-only `ReadDir` right only when `Path::is_file()` is true. Character devices are non-directories but are not regular files, so `/dev/null` retains `ReadDir`; Landlock rejects the rule before seven Python-script executor tests reach their intended assertions.

C002 must make access-right construction distinguish directories from non-directories. It must not weaken sandbox authority, bypass Landlock, broaden allowed paths, or turn this corrective pass into a sandbox redesign.

## 2. Current-state evidence

Repository state at planning baseline:

- PR #73 is merged to `main` as `8a556f05ab2f446ab8577f568bfb90912e49274e`.
- C001 is blocked, not closed, in `plans/closure/post-audit-correctness-simplification/009-corrective-status.md`.
- Hosted CI run `31266908787` failed seven Python-script executor tests after the earlier verification stages passed.
- The repeated sandbox setup error is: `add sandbox rule /dev/null: incompatible directory-only access-rights: BitFlags<AccessFs>(0b1000, ReadDir)`.
- `src/security/sandbox.rs::add_landlock_path_rule` builds read/write `AccessFs` rights and currently removes `ReadDir` only under `path.is_file()`.
- `/dev/null` is a special non-directory path, so the regular-file predicate is the wrong classification boundary for directory-only rights.
- `src/python_script/sandbox.rs` and `src/bin/codegg-sandbox-helper.rs` already carry the intended exact read/write path sets into `SandboxPolicy`; no helper protocol or policy-root redesign is indicated by this failure.
- The failure was exposed by the C001 hosted gate and was not introduced by C001's PR-metadata/merge reconciliation.

The transitive `lru 0.12.5` advisory has already been explicitly disposed as deferred upstream migration under C001. It is not C002 scope.

## 3. Required invariants

The implementation must preserve all of the following:

1. **Sandbox authority is unchanged.** C002 may correct which Landlock access bits are valid for an already-authorized path; it must not add read/write roots or grant additional filesystem authority.
2. **Directories retain directory semantics.** `ReadDir` remains available where the target is actually a directory and the existing policy permits reading it.
3. **Non-directories do not receive directory-only rights.** Regular files and special non-directory paths such as character devices must not carry `ReadDir` merely because they are not regular files.
4. **Read/write intent remains unchanged.** The correction must not convert a read-only path into writable access or otherwise broaden `AccessFs` beyond the existing policy intent.
5. **Failure remains fail-closed.** If the path kind cannot be determined safely, return contextual sandbox setup failure rather than silently omitting the rule or weakening restrictions.
6. **Existing path semantics remain intact.** Do not alter policy-root normalization, subprocess restrictions, helper request shape, scheduler/tool authority, or unrelated sandbox behavior.
7. **No CI bypass.** Do not ignore the seven tests, add platform skips to hide the failure, remove the hosted gate, or introduce permissive fallback behavior.
8. **No architecture expansion.** Keep the single daemon, single binary, one-job routine CI, and manual release policy unchanged.

## 4. Ordered implementation work

### WP1 — Lock the failing evidence

Before changing behavior:

- inspect hosted run `31266908787` and record the exact seven failing Python-script executor tests in the C002 closure record;
- reproduce the smallest representative failure on a supported Linux/Landlock environment when available;
- confirm the failure occurs during sandbox rule construction for `/dev/null`, before the test-specific executor assertion;
- confirm no newer `main` change has independently corrected or materially changed `add_landlock_path_rule`.

If the current source no longer matches this diagnosis, stop and reclassify the plan rather than applying a stale fix.

### WP2 — Correct path-kind access-mask construction

Make the smallest production change in `src/security/sandbox.rs`:

- classify the target on the semantic boundary Landlock requires: directory versus non-directory, not regular file versus everything else;
- retain directory-only rights only for actual directories;
- remove `ReadDir` from regular files and special non-directory targets such as `/dev/null`;
- preserve the existing read/write access set for all non-directory-compatible rights;
- keep ABI handling and `PathBeneath` construction unchanged unless current Landlock API behavior provides concrete evidence that a smaller compatible adjustment is required;
- add contextual error propagation for path-kind/metadata inspection if the selected implementation introduces a fallible metadata lookup;
- do not silently drop an authorized path from the ruleset to make Landlock accept the sandbox.

Prefer a small helper for access-mask/path-kind calculation only if it materially improves direct testing and avoids duplicating the rule logic. Do not create a generalized filesystem-type abstraction for this correction.

Symlink behavior is not a target of C002. Preserve current behavior unless a focused test demonstrates that the directory/non-directory correction cannot be made safely without resolving an existing symlink ambiguity; if that occurs, stop and report rather than broadening scope automatically.

### WP3 — Add focused regression tests

Add the minimum tests needed to pin the corrected invariant. Cover:

- a directory target: directory-compatible read rights still include the rights required by the existing policy;
- a regular file target: `ReadDir` is absent while valid file read rights remain;
- a special non-directory target on supported Linux, preferably `/dev/null`: `ReadDir` is absent and Landlock rule construction no longer rejects it;
- write-enabled non-directory handling if the existing helper currently grants `/dev/null` or another representative special path write authority;
- failure behavior for an unclassifiable/missing target if path-kind inspection becomes explicitly fallible.

Tests should assert the access-mask/path-kind contract as directly as practical, not merely assert that the helper process happened to exit successfully. Keep Linux/Landlock-specific tests narrowly gated to platforms where the production implementation itself is supported; do not use gating to hide a supported-host failure.

### WP4 — Re-run the concrete failing surface

Run, in increasing scope:

1. the focused `security::sandbox` path-kind tests;
2. the Python-script sandbox/executor tests corresponding to all seven failures in run `31266908787`;
3. the existing sandbox policy/static guards, including `scripts/check-sandbox-policy.py` and any still-applicable Bash sandbox ownership guard;
4. `scripts/verify.sh quick`;
5. the repository's existing single hosted `verify` job on the actual merge candidate.

Do not add another workflow lane, matrix, scheduled test, artifact gate, coverage gate, audit gate, or release check to prove this correction.

### WP5 — Strict closure reconciliation

If and only if WP4 is green:

- create `plans/closure/post-audit-correctness-simplification/010-sandbox-rights-correction-status.md`;
- record the exact production commit, the seven previously failing tests, focused verification, hosted run ID/result, and security/invariant review;
- mark C002 closed;
- reconcile C001 from blocked to closed because its only named hosted-verification blocker is resolved;
- update the corrective addendum and `plans/registry.md` so the post-audit correctness/simplification/footprint workstream returns to strict `closed` state;
- remove C002 from dependency-ready work and C001 from blocked work;
- preserve M001-M008 and the historical C001 record rather than rewriting their prior evidence.

If hosted verification fails for the same `/dev/null` rights issue, C002 remains active/blocked and must not be marked closed. If it fails for a clearly unrelated issue, classify that issue independently; do not automatically expand C002 into another general corrective audit.

## 5. Expected files and ownership

Primary expected production/test surface:

- `src/security/sandbox.rs`
- focused sandbox tests colocated with or directly exercising that module
- Python-script sandbox/executor tests only if a narrowly necessary regression assertion belongs there

Expected closure/planning surface after successful verification:

- `plans/closure/post-audit-correctness-simplification/010-sandbox-rights-correction-status.md`
- `plans/implementation/post-audit-correctness-simplification/011-sandbox-rights-correction-and-strict-closure.md`
- `plans/subsystems/post-audit-correctness-simplification-corrective-closure-addendum.md`
- `plans/registry.md`
- C001 status/closure text only where needed to reconcile the named blocker

Do not change manifests, dependencies, CI topology, release automation, protocol/storage formats, daemon ownership, TUI behavior, or unrelated executor architecture without concrete evidence that the narrow correction is impossible otherwise.

## 6. Compatibility and migration impact

No user-facing migration is expected.

C002 should not change:

- CLI or configuration syntax;
- daemon/client protocol;
- storage formats;
- public tool semantics;
- supported features;
- dependency versions or MSRV;
- binary topology or release process.

The intended runtime difference is only that valid authorized non-directory paths receive a Landlock-compatible rights mask instead of failing sandbox setup due to a directory-only bit.

## 7. Failure and recovery behavior

- Never recover from an incompatible rule by disabling Landlock or retrying without the affected path.
- Never convert a metadata/type-classification failure into implicit allow behavior.
- Preserve contextual errors identifying the path and sandbox operation.
- If the narrow rights correction causes a regression in directory access, file access, or sandbox authority, revert/refine the mask calculation rather than compensating with broader access.
- If Landlock 0.4.1 cannot represent the required special-file rule without changing the policy authority model, stop and document that finding; dependency/API migration requires separate review.

## 8. Security review requirements

The closure record must explicitly establish:

- allowed path sets are identical before and after the fix;
- no read-only path gained write authority;
- no sandbox rule is skipped for special files;
- directories retain required directory rights;
- non-directories lose only rights that are invalid because they are directory-only;
- executor subprocess restrictions and helper isolation remain unchanged;
- no test or CI weakening was used to obtain a green result.

## 9. Acceptance criteria

C002 is complete only when all of the following are true:

1. `/dev/null` no longer reaches Landlock rule construction with `ReadDir`.
2. The implementation uses a directory/non-directory classification appropriate to the rights being removed; it does not special-case only the literal `/dev/null` path.
3. Existing allowed path roots and read/write authority are unchanged.
4. Focused regression tests cover directory, regular-file, and special non-directory behavior.
5. All seven Python-script executor tests that failed in hosted run `31266908787` pass on the corrected merge candidate.
6. Relevant sandbox/static guards pass.
7. `scripts/verify.sh quick` passes.
8. The existing hosted `verify` job passes on the actual merge candidate.
9. No tests are ignored, weakened, or skipped merely to satisfy closure.
10. A C002 closure record names the exact implementation SHA and hosted run.
11. `plans/registry.md` and the corrective addendum are reconciled to strict closure only after criterion 8 is satisfied.
12. No unrelated dependency, CI, release, architecture, or feature work is introduced.

## 10. Stop conditions

Stop and report instead of expanding this plan if any of the following becomes necessary:

- weakening or disabling Landlock/sandbox enforcement;
- broadening an allowed path set or filesystem authority;
- changing supported-platform policy;
- redesigning the sandbox helper protocol or runtime-safety architecture;
- upgrading Landlock or another dependency as a prerequisite;
- changing CI topology or suppressing failing tests;
- reopening accepted M001-M008 production behavior;
- folding the independent supported-Linux Landlock fixture-evidence condition from the runtime-safety workstream into this corrective pass.

A newly discovered independent defect may justify a new narrowly scoped plan only after it is evidenced and classified.

## 11. Handoff checklist

Implementation agent should leave the repository with:

- [ ] the exact seven hosted failures captured from run `31266908787`;
- [ ] a narrow path-kind rights correction in `src/security/sandbox.rs`;
- [ ] focused directory/file/special-file regression coverage;
- [ ] all seven prior Python-script executor failures passing;
- [ ] sandbox/security guards passing;
- [ ] `scripts/verify.sh quick` passing;
- [ ] one normal hosted `verify` run green on the merge candidate;
- [ ] `plans/closure/post-audit-correctness-simplification/010-sandbox-rights-correction-status.md` created with exact evidence;
- [ ] C001/C002/addendum/registry reconciled to strict closed state only after hosted success;
- [ ] no CI, dependency, release, or product-scope expansion.
