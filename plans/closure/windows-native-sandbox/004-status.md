# Windows Native Sandbox Milestone 004 — Closure Status

Status: blocked

Source implementation plan:

- `plans/implementation/windows-native-sandbox/004-filesystem-token-and-acl-enforcement.md`

Source subsystem roadmap:

- `plans/subsystems/windows-native-sandbox-roadmap.md#7-milestones`

Repository baseline reviewed: `606f7a42cb90a54f0b9d74f49c6cf8801f7358c4`

Implementation commits or pull requests:

- `1903c523` — reusable AppContainer profile, process-attribute, and suspended launch helpers; test-only Rust token/Job proof
- `1ee041b` — cleanup hardening and verification-failure injection
- `3e33694`, `7f3d49f`, `606f7a4` — hosted launch-proof diagnostics and working-directory/flag investigations; the AppContainer Rust proof remains failing

## 1. Executive finding

M004 is blocked and is not closed. Windows fixture evidence validates an
AppContainer candidate, and the supervised launcher has limited test-only
helpers. The Rust AppContainer launch proof still fails with
`ERROR_INVALID_PARAMETER` at `CreateProcessW`; production filesystem enforcement,
ACL lease/recovery, and backend registration have not been implemented. The
required privileged isolated-VM matrix cannot run because only a hosted runner
is available.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| AppContainer access-check fixture for allowed roots and denied secret/outside writes | `research/windows-native-sandbox/appcontainer-access-proof.md`; hosted workflow runs through `38166050591` | partial | Disposable PowerShell fixture only; does not exercise production ACL ownership or launcher integration |
| Rust suspended AppContainer target has expected token SID and Job membership before resume | Hosted runs `38169853082` and `38173319891`; Rust AppContainer launch test logs | fail | `CreateProcessW` returns OS error 87 before token/Job assertions |
| Existing Windows process-launcher regression suite and suspended-target cleanup | Run `38173319891` | pass | Existing launcher step and failure-injection step completed successfully |
| Production backend applies and reports complete read/write enforcement | Source inspection of `src/security/sandbox` and `src/managed_process.rs` | not implemented | Constrained Windows launch remains unavailable; no Windows backend is registered |
| ACL lease cleanup/recovery under interruption and adversarial path cases | No privileged isolated VM is available | blocked | Required VM-only tests are not run |
| Credentials, IPC, inherited-handle and descendant adversarial denial | No production launch path or required matrix | not run | No claim of enforcement |

## 3. Production implementation evidence

`src/managed_process/windows.rs` contains testable AppContainer profile,
attribute-list, suspended process, and Job ownership primitives. They are not
connected to `ManagedProcessService` as a constrained production launch path.
The established hosted PowerShell fixture changes ACLs only beneath disposable
runner temporary roots and restores their before-images; it is not a
crash-recoverable production lease implementation. No Windows filesystem
backend is registered, and constrained Windows requests remain unavailable.

## 4. Verification executed

### Commands run

- `cargo fmt --all`
- `git diff --check`
- `python3 scripts/check_sandbox_contract.py`
- `python3 scripts/check_execution_ownership.py`
- Hosted workflow `38173319891` on Windows Server 2025

### Results

Formatting, whitespace, sandbox-contract, and execution-ownership checks passed
locally. In hosted run `38173319891`, the Windows managed-process launcher
suite and suspended-target failure-injection test passed. The Rust AppContainer
launch-ordering tests failed because `CreateProcessW` returned OS error 87,
even after using a null current directory and adding `CREATE_NO_WINDOW`. The
workflow stopped before its later native shell/policy checks. Earlier hosted
runs `38168149973` and `38168868303` also failed at `CreateProcessW` with an
explicit checkout or System32 current directory.

## 5. Invariant review

- Constrained Windows execution remains unavailable; no incomplete mechanism
  reports `filesystem=Enforced`.
- The existing Job-owned process path remains in place for unconstrained
  launches; its hosted launcher and failure-injection tests passed in run
  `38173319891`.
- No production ACL changes, privilege elevation, or fallback to broader
  authority were introduced.
- Unix sandbox behavior was not changed by this M004 follow-up.

## 6. Failure and recovery review

The test-only suspended-process wrapper terminates and waits for an unresumed
target before dropping its profile. This has not been verified on the failing
AppContainer launch path. Production ACL setup, restoration after normal exit,
cancellation, process crash, daemon restart, and contention have not been
implemented or qualified.

## 7. Migration and compatibility review

No storage or protocol migration was made. Existing constrained Windows
requests remain unavailable. No change to Unix compatibility is claimed by
this blocked record.

## 8. Security review

No production Windows read/write, credential, IPC, or network containment is
claimed. AppContainer and Job behavior observed in disposable fixtures is
candidate-mechanism evidence only. Production launch, exact ACL ownership and
recovery, and the required adversarial matrix remain outstanding.

## 9. Documentation and operations

- The M004 implementation plan records hosted run failures and the unavailable
  privileged VM requirement.
- The subsystem roadmap and registry identify M004 as blocked; constrained
  Windows execution remains unsupported.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | Rust AppContainer `CreateProcessW` proof returns error 87 | Rust-side suspended launch and token/Job verification cannot be established | Diagnose against the hosted Windows fixture/API contract and obtain a passing Rust proof before production integration |
| high | Production filesystem backend and crash-recoverable ACL leases are absent | Required Windows read/write restrictions and cleanup are not enforced | Implement the production lifecycle and behavioral probes without claiming partial enforcement as complete |
| high | Privileged isolated VM is unavailable | ACL interruption/recovery and adversarial matrix cannot be run | Resume when a privileged disposable Windows VM is available; run the required recovery and adversarial tests |

## 11. Roadmap disposition

M004 is blocked. Do not advance M005 or M006. Resume M004 when the hosted
AppContainer launch proof is diagnosable and the privileged isolated-VM
recovery/adversarial environment is available. M007 remains separately
blocked/optional; no MXC security conclusion is made.

## 12. Registry updates

- `plans/registry.md`: M004 is blocked on the hosted Rust launch proof and the
  unavailable privileged isolated-VM evidence; M005 and M006 stay gated.
- `plans/subsystems/windows-native-sandbox-roadmap.md`: M004 is blocked with
  the same evidence and environment blockers.
