# Post-Audit Correctness, Simplification, and Footprint — Corrective Closure Addendum

Status: closed

Source roadmap:

- `plans/subsystems/post-audit-correctness-simplification-roadmap.md`

Source closure:

- `plans/closure/post-audit-correctness-simplification/008-status.md`

Corrective records:

- C001: `plans/closure/post-audit-correctness-simplification/009-corrective-status.md`
- C002 target: `plans/closure/post-audit-correctness-simplification/010-sandbox-rights-correction-status.md`

Repository/PR state reviewed:

- merged implementation: `8a556f05ab2f446ab8577f568bfb90912e49274e`
- C002 planning baseline: `d0b62204a0740195c53face071635d44c147f12b`
- implementation PR: #73 (`planning/post-audit-correctness-simplification` -> `main`)
- final PR head: `4d105162ef39bbaa9a438e1f4b2d9060b10f3277`
- PR state: merged, ready for review, merge commit `8a556f05ab2f446ab8577f568bfb90912e49274e`
- blocking hosted run: `31266908787`

## 1. Why this addendum exists

M001-M008 production work is substantially complete and individually closed. At the initial C001 review, the workstream was marked strictly closed before its integration state matched that claim: PR #73 was outside `main` and its title/body described only the original M003 TUI slice.

C001 corrected the PR metadata and integration state and merged PR #73. Its required hosted gate then exposed a pre-existing sandbox-rights defect: seven Python-script executor tests failed during sandbox setup because `/dev/null` received directory-only `ReadDir` rights. C002 corrected that defect and completed strict closure.

The remaining corrective implementation is C002:

- `plans/implementation/post-audit-correctness-simplification/011-sandbox-rights-correction-and-strict-closure.md`

The earlier registration stub is superseded and retained only for history:

- `plans/implementation/post-audit-correctness-simplification/010-sandbox-file-rights-correction.md`

C002 is not a new product milestone. It owns one concrete sandbox invariant defect and the strict closure reconciliation that becomes possible after that defect passes the repository's normal hosted verification.

## 2. Historical C001 scope and disposition

C001 owned:

- making PR #73 title/body accurately describe the integrated M001-M008 workstream;
- reconciling draft/ready-for-review state with actual merge readiness;
- confirming the relevant PR verification state;
- merging the completed work to `main` when repository state permitted;
- confirming the merge result contains the accepted M001-M008 production tree;
- reconciling registry, addendum, and closure records to the actual merged SHA;
- explicitly disposing the pre-existing transitive `lru` advisory without forcing a broad dependency migration.

Those integration/advisory tasks are complete. The `lru` advisory was recorded as deferred upstream migration and is not C002 scope.

C001 was blocked solely because its strict exit criteria required a successful normal hosted `verify` result and run `31266908787` exposed the concrete `/dev/null` sandbox-rights defect. C002 corrected that defect and run `31425564638` passed on the actual merge candidate.

## 3. C002 scope

C002 owns only:

- confirming the seven hosted failures and current source diagnosis;
- correcting Landlock access-mask construction so directory-only rights are retained only for directories;
- ensuring regular files and special non-directory targets such as `/dev/null` do not receive `ReadDir`;
- adding focused regression coverage for directory, regular-file, and special-file path kinds;
- rerunning the seven failed executor tests, existing sandbox/security guards, `scripts/verify.sh quick`, and one normal hosted `verify` run;
- creating the C002 closure record and reconciling C001/C002/the registry to strict closure after hosted verification passed.

C002 does not own dependency upgrades, broad Landlock redesign, new sandbox capabilities, CI topology changes, release automation, or a second audit of accepted M001-M008 code.

The independent supported-Linux Landlock fixture evidence condition in the runtime-safety roadmap remains separate. It must not be folded into C002.

## 4. Invariants

- The accepted M001-M008 production implementation must not be rewritten merely to close this corrective record.
- Sandbox authority and allowed path sets must not be broadened.
- Directory-only Landlock rights must be decided on a directory/non-directory boundary, not by treating every non-regular-file path as a directory.
- Read-only paths must remain read-only; no access bit may be added merely to make a rule acceptable.
- A special-file rule must not be silently skipped as a recovery mechanism.
- Path-kind/metadata failures must fail closed with context.
- Existing helper protocol, subprocess restrictions, policy-root behavior, daemon authority, scheduler ownership, and protocol/storage compatibility remain unchanged.
- Single-binary topology, manual release cadence, and one-job routine CI remain unchanged.
- No new CI lane, matrix, release workflow, dependency bot, audit gate, artifact bundle, coverage gate, or binary-size threshold may be introduced.
- The workstream returned to strict `closed` after C002 passed the normal hosted `verify` job on its actual merge candidate.

## 5. Dependency and execution order

```text
M001-M008 closed implementation
        |
        v
C001 integration/advisory work complete, strict closure achieved by C002
        |
        v
C002 sandbox path-kind rights correction
        |
        v
normal hosted verify green
        |
        v
C001 + C002 reconciled; workstream strictly closed
```

Remaining execution order:

1. Capture the exact seven failures from hosted run `31266908787` and confirm the current `src/security/sandbox.rs::add_landlock_path_rule` diagnosis still applies.
2. Make the smallest directory/non-directory access-mask correction; do not special-case only `/dev/null` and do not broaden sandbox authority.
3. Add focused directory, regular-file, and special-file regression tests and rerun all seven previously failing Python-script executor tests.
4. Run existing sandbox/security guards and `scripts/verify.sh quick`.
5. Run one existing hosted `verify` job on the actual merge candidate.
6. Completed: created `plans/closure/post-audit-correctness-simplification/010-sandbox-rights-correction-status.md`, marked C002 and C001 closed, removed active/blocked rows, and returned this workstream to strict `closed` state.

No PR #73 metadata, draft-state, merge, or advisory-disposition work remains. Those steps are historical C001 work and must not be repeated by the C002 implementation agent.

## 6. Advisory disposition

The transitive `lru 0.12.5` advisory recorded during M008/C001 has already been classified from the locked path `codegg -> ratatui 0.29.0 -> lru 0.12.5`. The compatible patched line requires a broader upstream dependency migration than this closure pass permits, so C001 recorded deferred upstream migration.

C002 must not reopen that disposition. A future dependency-maintenance plan may be registered only if independent risk or product priority warrants it.

## 7. C002 exit conditions

This addendum returns to strict closed status only when:

- the `/dev/null` rule no longer carries incompatible `ReadDir` rights;
- the fix is semantic for non-directories rather than a literal-path exception;
- allowed paths and read/write authority remain unchanged;
- focused directory/file/special-file regression tests pass;
- all seven tests that failed in hosted run `31266908787` pass on the corrected merge candidate;
- existing sandbox/security guards pass;
- `scripts/verify.sh quick` passes;
- the normal existing hosted `verify` job succeeds on the actual merge candidate;
- no test, platform support, or CI gate was weakened to obtain that result;
- the C002 closure record names the exact implementation SHA and hosted run;
- `plans/registry.md` no longer lists C001 as blocked or C002 as ready/active;
- M001-M008 historical closure records and C001 integration history remain intact.

## 8. Stop conditions

Stop and report rather than broadening C002 if:

- fixing the defect requires weakening or disabling Landlock enforcement;
- the allowed path set or filesystem authority would need to expand;
- the Landlock dependency must be upgraded or replaced as a prerequisite;
- the sandbox helper protocol or broader runtime-safety architecture must change;
- supported-platform policy would need to change;
- CI tests must be skipped, ignored, or removed to pass;
- CI topology or release policy would need to change;
- accepted M001-M008 production behavior must be reopened;
- the independent runtime-safety supported-Linux fixture-evidence condition is mistaken for a prerequisite of this corrective pass.

A newly discovered independent defect gets separate classification and, only if justified, a separate narrow plan. Do not turn C002 into another general correctness or verification program.
