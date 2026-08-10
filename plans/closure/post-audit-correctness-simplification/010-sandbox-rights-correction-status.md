# Post-Audit Correctness, Simplification, and Footprint C002 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/post-audit-correctness-simplification/011-sandbox-rights-correction-and-strict-closure.md`
Source subsystem roadmap: `plans/subsystems/post-audit-correctness-simplification-corrective-closure-addendum.md`
Repository baseline reviewed: `d0b62204a0740195c53face071635d44c147f12b`
Implementation commits: `6958703c` (rights correction), `2f09ee79` (Linux Clippy correction), `12921edb`/`d16da6e8` (diagnostic evidence), `855de301` (faithful Landlock integration assertion)
Hosted verification: [run 31425564638](https://github.com/dbowm91/codegg/actions/runs/31425564638) on `855de301cfbd3f533b392245aa7808b215490bf4`

## 1. Executive finding

C002 is strictly closed. Landlock path access construction now classifies paths by
directory versus non-directory metadata. Directory-only rights remain on directories;
regular files and special non-directory paths such as `/dev/null` receive the
Landlock-compatible file mask, so `/dev/null` no longer receives `ReadDir` or rejects
ruleset construction. The existing hosted Ubuntu verification passed all gates and the
full workspace test suite, including the seven Python executor tests that blocked C001.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Confirm the seven historical failures | Hosted run `31266908787` recorded all seven names and the `/dev/null` `ReadDir` error | satisfied |
| Directory/non-directory correction | `src/security/sandbox.rs::landlock_access_for_path` uses fallible metadata classification | satisfied |
| Directory rights preserved | `landlock_access_keeps_directory_rights_for_directories` | satisfied; hosted |
| Regular-file rights corrected | `landlock_access_removes_directory_rights_for_regular_files` | satisfied; hosted |
| Special-file rights corrected | `landlock_access_removes_directory_rights_for_special_files` covers `/dev/null`, including writable file rights | satisfied; hosted |
| Classification failure fails closed | `landlock_access_fails_closed_when_path_cannot_be_classified` | satisfied; hosted |
| Seven blocked Python executor tests | Hosted run `31425564638` workspace tests passed | satisfied |
| Existing sandbox integration contract | Hosted `sandbox_landlock` passed, including read-only/write/outside-root and helper-status tests | satisfied |
| Existing guards and quick verification | Hosted guards/Clippy plus local `scripts/verify.sh quick` passed | satisfied |
| Normal hosted gate on actual candidate | Run `31425564638`, SHA `855de301…` | satisfied |

## 3. Production implementation evidence

`apply_landlock` now obtains path metadata before constructing each `PathBeneath` rule.
Actual directories retain the requested directory-compatible mask. Every non-directory
uses `AccessFs::from_file(ABI::V1)`, removing directory-only rights without adding any
read or write authority. Metadata errors include the path and return setup failure.

No allowed path root, helper protocol, subprocess restriction, policy root, scheduler
authority, storage format, or feature surface changed.

## 4. Verification executed

- Local focused Python executor tests: 20 passed, including all seven historical names.
- Local `python3 scripts/check_sandbox_contract.py`: passed.
- Local `scripts/verify.sh quick`: passed.
- Hosted run `31425564638`: setup, agent validation, core-boundary guard, sandbox contract
  guard, execution-ownership guard, formatting, Clippy, and the full workspace tests all
  passed on Ubuntu. The `sandbox_landlock` integration tests passed, including the actual
  non-directory `/dev/null` rule path.
- The integration assertion was corrected to use actual shell file opens rather than
  `test -r` metadata checks; this strengthens the existing sandbox contract and does not
  skip or weaken any test.

## 5. Invariant review

- Allowed path sets are unchanged before and after the correction.
- Read-only paths did not gain write authority.
- No authorized special-file rule is skipped.
- Directories retain `ReadDir`; non-directories lose only directory-only rights.
- Unknown path classification fails closed.
- Landlock remains a hard requirement and is never bypassed.
- Single-daemon, single-binary, one-job routine CI, and manual release policy are unchanged.

## 6. Failure and recovery review

The prior incompatible `/dev/null` rule now fails neither during setup nor by fallback.
There is no retry with a weaker mask, rule omission, Landlock disablement, or permissive
recovery path. The only test adjustment replaces an `access(2)`-style metadata check with
real open attempts so the integration test measures Landlock enforcement directly.

## 7. Migration and compatibility review

No migration is required. CLI/configuration syntax, daemon/client protocol, storage,
public tool behavior, supported features, dependencies, MSRV, binary topology, and
release process are unchanged.

## 8. Security review

The correction is semantic for all non-directories and is not a `/dev/null` exception.
It removes only rights that Landlock defines as directory-only or incompatible with a
non-directory target. Directory access and valid file read/write rights remain governed
by the original policy masks. Helper isolation and subprocess restrictions remain
unchanged. No CI bypass, platform skip, or test suppression was used.

## 9. Documentation and operations

The C002 implementation plan is marked implemented. C001, the corrective addendum, and
the registry are reconciled to strict closure. The historical M001-M008 records and the
blocked C001 evidence record remain preserved as history.

## 10. Unresolved findings (severity: critical/high/medium/low)

- None introduced by C002; no critical, high, or medium finding remains.
- Low/deferred: the previously recorded transitive `lru 0.12.5` advisory remains deferred
  upstream migration under C001 and is outside C002 scope.
- External/operational: the independent supported-Linux Landlock fixture evidence in the
  runtime-safety workstream remains conditionally closed under its own record; it does not
  block or reopen this corrective closure.

## 11. Roadmap disposition

C001 is reconciled from blocked to closed because its only named hosted-verification
blocker is resolved by C002. C002 is closed. The post-audit correctness, simplification,
and footprint corrective addendum returns to strict `closed`.

## 12. Registry updates

- Added this C002 closure record and recorded the exact implementation and hosted SHA.
- Removed C002 from dependency-ready and active closure work.
- Removed C001 from blocked work and recorded C001 as closed under recently closed work.
- Marked the post-audit corrective subsystem line `closed`.
- Audited all remaining registry blocked work and affected dependency graphs: no other
  registered future plan listed C001 or C002 as a hard/interface dependency, so no future
  plan became newly `ready`.
- Preserved M001-M008 and the historical C001 record without rewriting their prior evidence.
