# Windows Native Sandbox C054 — Closure Status

Status: closed (no candidate; no production admission)
Source implementation plan: `plans/implementation/windows-native-sandbox/054-windows-sandbox-successor-disposition-and-handoff.md`
Source subsystem roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md`
Repository baseline reviewed: `39f149a15ef529d66787d1d11551872ea22ec384`
Implementation commits: `829ef60e` — exact-SHA C050/C052 hosted evidence; this closure records the no-candidate disposition

## 1. Executive finding

No candidate qualifies for the next Windows native sandbox implementation.
C050's WRITE_RESTRICTED child could not be created (`CreateProcessWithTokenW`
returned access denied); both C052 token-flag variants were denied by
`CreateProcessAsUserW` before child entry. C053 found no qualifying MXC SDK/API
delta. C049's broad-read dossier is decision-ready but ADR-0030 remains
proposed and unaccepted. C051's hard positive prerequisite was not met.

Close this line as a bounded no-candidate disposition. Do not create another
candidate or repeat the failed probes under the same hosted-runner conditions.
Windows `SandboxRequest::Required` remains unavailable and fail-closed.

## 2. Requirement-to-evidence matrix

| Requirement / operation | Evidence | Result | Notes |
|---|---|---|---|
| C049 broad-read contract and authorization alternatives | C049 closure and `research/windows-native-sandbox/c049-broad-read-decision-pack.md` | pass as decision artifact | ADR-0030 remains proposed; no profile authority |
| Restricted child first instruction | C050/C052 hosted run `38615233054` | blocked | Both child-creation APIs returned access denied; no child entered |
| Host-read confidentiality | No child-level read probe | not run | Broad reads remain a known property/risk of this candidate class, not a CodeGG guarantee |
| Allowed stage write / denied outside write | No C050/C052 child operation | not run | C050 was blocked before child creation |
| Outside canary and synthetic DACL preservation | C052 denied-launch assertions in `38615233054` | pass, limited | Test-owned fixture bytes and ACL snapshots unchanged; no user worktree involved |
| Alias paths, hardlinks, ADS, reparse points, TOCTOU | No child-level adversarial test | not run | No denial claim |
| Inherited/duplicated handles, privileged IPC, process memory | No child created | not run | Unqualified |
| Job ownership, descendants, cancellation, teardown | No C050/C052 child created | not run | Unqualified for these candidates |
| Network sockets / WFP | No network probe | not run | No Offline guarantee |
| Git/Python/Cargo compatibility and overhead | C051 did not run | blocked | C050 hard positive gate not met |
| MXC strict candidate | C053 closure | fail / no candidate | Published typed API/source delta gate did not authorize native re-entry |
| Production Required and process-surface coverage | C047/C048 closures and current backend registry | blocked | Test runner/MCP/LSP surfaces remain outside complete enforced integration; C048 admission conditions are not met |

## 3. Production implementation evidence

No production backend, profile, approval path, process routing, persistent
identity, WFP rule, installer, schema, or release behavior was added. The
changes in this line are decision documents and feature-gated test probes only.
The test-support release guard passed. Production Windows Required behavior
is unchanged and remains unavailable.

## 4. Verification executed

### Commands run

```bash
rtk cargo fmt --all -- --check
rtk git diff --check
rtk python3 scripts/check_adr_integrity.py
rtk python3 scripts/check_sandbox_contract.py
rtk python3 scripts/check_execution_ownership.py
rtk python3 scripts/check_windows_test_support_release.py
```

### Results

- All listed local checks passed on the final implementation revision before closure documentation.
- `rtk bash scripts/verify.sh quick` passed earlier in the implementation turn; final code differences after that run were limited to test-only Windows diagnostic imports/context, and focused guards plus hosted Windows compilation covered those updates.
- Hosted run [`38615233054`](https://github.com/dbowm91/codegg/actions/runs/38615233054), exact SHA `829ef60e36080f78514f83ef77820678811f5711`: C050 compiled then failed with access denied at `CreateProcessWithTokenW`; C052 ran independently and recorded access denied for both `CreateProcessAsUserW` variants (`flags=0` and `WRITE_RESTRICTED`). Its DACL/canary invariance assertions passed, but no child behavior was observed. The overall workflow failed at C050 as expected for the blocked run.
- Runner: Windows Server 2025 Datacenter, build `26100`, UBR `33438`, x64; Rust `1.89.0`; `runneradmin`, administrator role true, high-integrity token true, UAC enabled.
- The preceding run `38613772082` exposed one missing test-only `TerminateProcess` import. It was corrected in `3e8d77e7` before the final run. An earlier run `38611777692` had import/scope compile errors and was corrected before `74ddf655`.
- Ordinary Windows launcher suites were skipped on the manual opt-in run; only C050/C052 probes and the AppContainer filesystem proof executed.

## 5. Invariant review

No strict-profile downgrade, FullHost fallback, parent-to-child authority
expansion, or test-support release exposure was introduced. The C050/C052
fixtures targeted fresh temporary roots; no populated user worktree ACL,
owner, SACL, MIC, or content was intentionally modified. Since neither
restricted child was created, no candidate confinement invariant was proven.

## 6. Failure and recovery review

C050 and both C052 variants stopped before a child process handle was
returned. The Job-before-resume, child teardown, descendant, cancellation,
restart, partial promotion, and recovery paths were not exercised. C052
verified only that denied launches left its synthetic fixture DACLs and
outside-canary bytes unchanged. No retry, privilege grant, or ACL relaxation
was attempted.

## 7. Migration and compatibility review

No migration, protocol, public profile, compatibility alias, or installer was
added. ADR-0030 is still proposed; existing strict profile representations
retain their prior meaning. No stage/promotion contract is authorized, and
C046 remains blocked.

## 8. Security review

The hosted-runner access-denied result is a setup limitation, not a filesystem
denial. No claim is made for broad-read write confinement, secret reads,
aliases, handles, IPC, process memory, network isolation, descendant control,
or native tool compatibility. No candidate is ready for an independent
production security review. Any future candidate must receive a separately
accepted production architecture decision and fresh child-level evidence.

## 9. Documentation and operations

Closure records C050, C051, C052, and C053 describe each bounded result.
The C052 research record contains upstream revision/license pins and paired
run outcomes. Registry and subsystem roadmap now record C049–C054 dispositions.
The exact GitHub Actions run is linked above; no new permanent CI lane was
added.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | Neither bounded token-launch probe could create a child on the available hosted Windows runner | No broad-read/write-confined or launch-context candidate is qualified | Keep this successor line closed with no candidate; require a new explicit authorization and suitable environment before any new native probe |
| high | C045/C046/C048 gates remain unmet and C047 records direct-spawn bypasses | Identity/WFP, staging/promotion, and complete process admission remain unavailable | Preserve their independent blocked status; do not infer their closure from C054 |
| medium | Maintainer accept/reject decision on ADR-0030 remains outstanding | Broad-read compatibility profile has no authorization | Maintainers may disposition the C049 decision packet independently; this does not authorize a backend |

## 11. Roadmap disposition

C054 closes **no candidate / no admission**. Do not open C045 provisioning,
C046 staging/promotion, or C048 production admission from this evidence. Do
not revise C048's strict-positive prerequisites to admit a broad-read
candidate. A future architecture choice requires new explicit authorization,
candidate-specific child-level evidence, complete process-surface coverage,
and independent review. Windows Required remains unavailable.

## 12. Registry updates

Mark C054 closed no-candidate and link this record in `plans/registry.md` and
`plans/subsystems/windows-native-sandbox-roadmap.md`. Keep C050/C052 closed
blocked, C051 closed blocked, C053 closed negative, ADR-0030 proposed, and
C045/C046/C048 blocked. No dependency is unblocked.
