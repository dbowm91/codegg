# Windows Native Sandbox C039 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/windows-native-sandbox/039-cross-branch-adr-identity-reconciliation.md`
Source subsystem roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#post-adr-0026-pr-111-mainline-reconciliation`
Repository baseline reviewed: feature `33e73915bb1789511d3fb7829141de05870d2325`; `origin/main` `b9ba494c1b21638c2e2f660301ba207ce8859682`; common base `8535f6d7c2e68f5d134ba0c73bd146ca8bc0bd03`
Implementation commits: `da4eabb0825dd8429878f3e44fc9ca5ed6988b96` — canonical Windows ADR identity migration, provenance aliases, and integrity guard

## 1. Executive finding

C039 is closed as an identity-only documentation and governance correction.
The mainline TUI decisions retain ADR-0021 and ADR-0022. The four Windows
decisions now use ADR-0023 through ADR-0026. The former files remain byte-for-
byte aliases under `plans/archive/windows-adr-id-aliases/`, with their source
branch SHA and content fingerprints recorded in
`plans/adrs/windows-adr-id-migration.md`.

The migration changes no decision status, security evidence, architecture, or
execution authority. C029/C034/C038 remain no-admission; C030/C031 remain
terminally blocked. Windows production `SandboxRequest::Required` is still
unavailable and filesystem/network `Enforced` remains unclaimed.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Allocate globally unique IDs in the proposed combined tree | `scripts/check_adr_integrity.py --self-test` builds synthetic mainline TUI ADR-0021/0022 plus Windows ADR-0023–0026 and checks uniqueness | Pass |
| Preserve prior Windows records and provenance | Four archived aliases match `git show 631ae9488f251ac70e91b9c8299d2c1c3bff83fa:plans/adrs/<original>` byte-for-byte; SHA-256 values in migration record | Pass |
| Preserve decisions, status and evidence | Canonical copies normalize exactly to original contents after reversing only ID/title/path mapping | Pass |
| Update Windows-scoped references without changing TUI meaning | Windows roadmap, implementation/closure records, registry and research links now use canonical Windows IDs; TUI ADR-0021/0022 references remain unchanged | Pass |
| Detect duplicate IDs and dangling local ADR paths | `scripts/check_adr_integrity.py` runs synthetic collision/migration/archive fixtures and scans the active repository | Pass |
| Preserve fail-closed security posture | C029/C034/C038 dispositions, terminal C030/C031 blocks, and production unavailable state unchanged | Pass |

## 3. Production implementation evidence

No production code changed. New canonical ADR records are identity-adjusted
copies; archived aliases preserve the exact old records. The mapping and
provenance are recorded in `plans/adrs/windows-adr-id-migration.md`, and the
ADR directory README documents that the aliases are not active allocations.

`scripts/check_adr_integrity.py` checks unique numeric IDs among active ADR
filenames and verifies repository-local `plans/adrs/ADR-*.md` references. Its
synthetic fixture covers a merged TUI/Windows ADR list, a duplicate allocation,
a dangling link, and an archived alias containing a historical path-like
string.

## 4. Verification executed

- `bash scripts/verify.sh quick` — passed, including formatting, repository
  guards, ADR integrity check, and workspace check.
- `python3 scripts/check_adr_integrity.py` — passed on the feature tree.
- `python3 scripts/check_adr_integrity.py --self-test` — passed synthetic
  merged-tree, collision, dangling-link and archived-alias cases.
- Provenance normalization script comparing original Git blobs, archived
  aliases and canonical records — passed for all four ADRs.
- `git diff --check` — passed.

Hosted CI for this implementation commit is part of C040's exact integrated-
head verification; C039 makes no claim about that later run.

## 5. Invariant review

- Mainline TUI ADR-0021 and ADR-0022 identities remain reserved and untouched.
- Windows ADR dispositions and substantive content are unchanged.
- Historical aliases are outside the active `plans/adrs/ADR-NNNN-*.md`
  namespace and remain reproducible from the recorded source SHA.
- Windows `SandboxRequest::Required` remains fail-closed/unavailable; no
  filesystem or network `Enforced` claim is introduced.

## 6. Failure and recovery review

The migration is a reversible set of document renames/copies. The source
records remain available at their original source commit and in the archive;
the migration table maps each former identity to its new canonical identity.
No runtime recovery semantics are affected.

## 7. Migration and compatibility review

All active Windows-scoped references now target ADR-0023 through ADR-0026.
Mainline TUI references to ADR-0021/0022 retain their meanings. There is no
protocol, storage, command, or runtime compatibility effect.

## 8. Security review

This change adds no sandbox mechanism or authority. The negative/no-admission
evidence and the terminally blocked broker/guest alternatives remain intact.
The checker validates document identity and links only; it does not infer ADR
acceptance or security qualification from numbering.

## 9. Documentation and operations

Updated `plans/adrs/README.md`, the Windows roadmap, registry, and
Windows-scoped references. Added migration provenance and the deterministic
integrity guard to both `scripts/verify.sh quick` and the existing hosted CI
job. No new CI lane was added.

## 10. Unresolved findings

None for C039. C040 remains the required integration gate for reconciling
current `main`, validating the actual merged tree and registry, and running
hosted verification on the integrated exact SHA.

## 11. Roadmap disposition

C039 is closed. C040 becomes eligible and remains the next Windows integration
milestone. Shell-policy C001 remains dependent on integrated exact-head
verification under C040. No Windows sandbox capability or downstream security
milestone is unblocked.

## 12. Registry updates

The registry records C039 closed with this closure and C040 ready for
integration. The dependency audit found C040 is the sole hard dependent newly
unblocked by C039; it does not unblock M004/C011, C030/C031, or any production
Windows sandbox capability.
