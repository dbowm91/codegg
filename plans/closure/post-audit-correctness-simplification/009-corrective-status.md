# Post-Audit Correctness, Simplification, and Footprint C001 — Corrective Closure Status

Status: closed
Source implementation plan: `plans/implementation/post-audit-correctness-simplification/009-corrective-pr-integration-and-advisory-cleanup.md`
Source subsystem roadmap: `plans/subsystems/post-audit-correctness-simplification-corrective-closure-addendum.md`
Repository baseline reviewed: `8a556f05ab2f446ab8577f568bfb90912e49274e`
Implementation commits: PR #73 merge `8a556f05ab2f446ab8577f568bfb90912e49274e`

## 1. Executive finding

PR #73 was corrected and merged to `main` without production changes beyond the accepted
M001-M008 tree. C001 is now strictly closed: C002 corrected the concrete `/dev/null`
sandbox-rights defect and the normal hosted `verify` gate passed on the corrected merge
candidate. The C002 evidence is recorded in
`plans/closure/post-audit-correctness-simplification/010-sandbox-rights-correction-status.md`.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Accurate PR metadata | [PR #73](https://github.com/dbowm91/codegg/pull/73), final title/body | satisfied |
| Ready and merged integration | PR #73 marked ready; merged as `8a556f05…` | satisfied |
| Accepted M001-M008 tree on `main` | merge commit contains the branch head `4d105162…` | satisfied |
| Normal hosted `verify` gate | [C002 run 31425564638](https://github.com/dbowm91/codegg/actions/runs/31425564638) on `855de301…` | satisfied |
| Explicit `lru` disposition | locked path `codegg → ratatui 0.29.0 → lru 0.12.5`; RustSec `RUSTSEC-2026-0002` patched at `>=0.16.3` | D2 deferred upstream migration |
| Final strict closure | C002 closure record and reconciled registry | satisfied |

## 3. Production implementation evidence

No C001 source or manifest change was made. PR metadata now describes bounded/SSRF-safe
HTTP, daemon identity/JSON correctness, TUI fixes, dependency slimming, CI contraction,
stack-root correction, execution-model cleanup, and final evidence. The single-daemon,
single-binary, manual-release architecture is unchanged.

## 4. Verification executed

- `git diff --check`: passed on the clean working tree.
- Existing PR `verify` runs repeatedly passed setup, agent validation, boundary/security
  guards, formatting, and Clippy, then failed 7 tests after 4,159 passed.
- Failure: `sandbox setup failed: add sandbox rule /dev/null: incompatible directory-only
  access-rights: BitFlags<AccessFs>(0b1000, ReadDir)`.
- The same seven failures are present on pre-C001 documentation heads, so this is not
  introduced by PR metadata or the merge reconciliation.
- C002 corrected the issue; the actual merge-candidate run [31425564638](https://github.com/dbowm91/codegg/actions/runs/31425564638)
  passed the full workspace test suite.

## 5. Invariant review

Accepted M001-M008 production behavior was not reopened. No new CI lane, audit gate,
release automation, dependency bot, size threshold, or coverage/benchmark gate was added.

## 6. Failure and recovery review

The seven tests exercised Python-script sandbox enforcement and initially failed before
their intended assertions. C002 corrected the path-kind mask and added focused coverage.

## 7. Migration and compatibility review

No storage, protocol, CLI, config, endpoint, feature, packaging, or MSRV change occurred.
No migration is required.

## 8. Security review

The failure concerned filesystem sandbox rights. C001 did not weaken or bypass the
restriction, and C002 preserved the authority boundary while distinguishing directories,
regular files, and special files.

## 9. Documentation and operations

PR #73 metadata and the planning records are reconciled to the merged SHA. The advisory is
documented as deferred upstream migration; no broad Ratatui migration was attempted.

## 10. Unresolved findings

- **High/medium:** none remain from C001/C002.
- **Low:** the transitive `lru 0.12.5` advisory requires upstream Ratatui migration; current
  RustSec metadata marks the relevant soundness advisory informational and patches it at
  `lru >=0.16.3`.
- **External/operational:** independent supported-Linux Landlock evidence remains under its
  own runtime-safety closure and was not reopened by C001.

## 11. Roadmap disposition

C001 is strictly closed by the C002 correction and hosted verification. No other future
plan became unblocked because no registered plan listed C001 or C002 as a remaining
hard/interface dependency.

## 12. Registry updates

- PR #73 is merged and removed from PR integration work.
- C001 is recorded as closed after the C002 sandbox correction and successful hosted
  verification.
- C002 is recorded as closed by `plans/closure/post-audit-correctness-simplification/010-sandbox-rights-correction-status.md`.
- No unrelated blocked plan became dependency-ready.
