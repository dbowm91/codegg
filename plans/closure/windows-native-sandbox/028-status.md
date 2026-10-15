# Windows Native Sandbox C028 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/windows-native-sandbox/028-token-launch-initialization-diagnostic.md`

Source subsystem roadmap:

- `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0021-launch-authority-decision-gate`

Repository baseline reviewed: `7a27bc7e6566616fc1b2b8a69637f12050d42843`; tested branch head: `33c82f56039df173fe04f8aabc1a863fa225f006`.

Implementation commits:

- `37b76d32` — bounded PowerShell launcher phase trace and native CRT/no-CRT marker.
- `7d654816`, `b0a12302`, `33c82f56` — hosted linker corrections and negative report collection.

## 1. Executive finding

C028 isolates the C027 `CreateProcessWithTokenW` timeout to the period before the child's first user instruction on the hosted Windows Server 2025 runner. All four verified primary-token children resumed from suspend count 1, belonged to their kill-on-close Job, and then timed out without either a CRT `main` marker or a custom no-CRT entry marker. The same outcome with and without `LOGON_WITH_PROFILE` means profile loading and CRT startup do not explain this observed timeout. The result does not identify the exact Windows loader or image-initialization substage.

The hosted phase is conditionally closed with a negative result. The privileged `CreateProcessAsUserW` comparison remains unrun because C028 requires a separately consented disposable Windows VM for that test. Windows production constrained execution remains unavailable.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Capture previous `ResumeThread` count | Workflow `38396698509`, exact branch SHA `33c82f56` | pass | Each of four cases returned previous suspend count 1. |
| Verify primary token and Job before resume | Same workflow report | pass | User SID matched the disposable principal; `TokenPrimary` and `IsProcessInJob` checks passed in session 2. |
| Locate first-instruction phase | CRT and custom no-CRT marker cases | negative / phase isolated | Zero of four cases reached either user-entry marker; each timed out at 15 seconds before user entry. |
| Compare profile loading | `CreateProcessWithTokenW` flags 0 and `LOGON_WITH_PROFILE` | negative | Both profile cases had the same pre-entry timeout; HKU state could not be observed because user code did not start. |
| Capture desktop, environment and stdio context | Child context marker in native fixture | partial | The probe held the default desktop, target-token environment block and unredirected stdio constant. No in-child context was emitted because no case reached user entry. |
| Privilege-enabled `CreateProcessAsUserW` comparison | Not run | blocked | Requires a separately consented disposable Windows VM. C027's hosted control returned Win32 1314; this does not substitute for the VM comparison. |
| Cleanup | Workflow `38396698509` | pass | Disposable principal removal succeeded; fixture root was deleted. |

## 3. Production implementation evidence

The test-only harness is `scripts/windows_token_initialization_trace.ps1`; its native fixture is `scripts/windows_token_initialization_marker.c`. The workflow adds a separate bounded `token-initialization-trace` job to `.github/workflows/windows-dedicated-user-feasibility.yml`. The harness creates one unique disposable principal and per-case directories, verifies the suspended child's user SID, primary-token type and Job membership, records process/token session IDs and previous suspension count, then resumes and waits at most 15 seconds before terminating the owned Job. It passes no inherited handles and touches no original user ACLs.

No launcher or sandbox production code changed. No restricted-token child was launched and no host-read denial was tested.

## 4. Verification executed

### Commands and hosted evidence

- `bash scripts/verify.sh quick` — passed.
- `python3 scripts/check_sandbox_contract.py` — passed.
- `python3 scripts/check_execution_ownership.py` — passed.
- PowerShell parser for `scripts/windows_token_initialization_trace.ps1` — passed.
- MinGW compile of `windows_token_initialization_marker.c` with `-Wall -Wextra -Werror` — passed.
- Hosted Windows workflow `38396698509`, source SHA `33c82f56039df173fe04f8aabc1a863fa225f006` — report collection passed; all four launch cases recorded bounded pre-user-entry timeouts; C027 regression jobs and cleanup passed.
- Earlier hosted attempts `38395828818` and `38396050132` stopped during no-CRT fixture linking before running child probes. The linker subsystem and no-CRT stack-probe dependency were corrected before the final measured run.

## 5. Invariant review

- No host-wide privilege was enabled.
- No original profile or host file ACL was changed.
- The child token and Job were checked before each resume.
- Every wait was bounded and timeout cleanup terminated the Job.
- No read-denial, network-isolation or production-admission claim is made.

## 6. Failure and recovery review

Four unique child processes were independently bounded to 15 seconds. Each timeout was terminated through its owned Job. The disposable local account and fixture root were removed. No service, persistent identity, reboot, or restart behavior was introduced or claimed.

## 7. Migration and compatibility review

No API, schema, configuration, installer, MSRV, or Unix behavior changed. The workflow is test-only and Windows-hosted.

## 8. Security review

The test principal and fixture directories are per-run. The native probe writes only inside those owned directories. No token is persisted, no privileged handles are inherited, and the hosted runner's existing privileges were only observed. The timeout remains negative evidence about this launch path, not proof of a sandbox boundary.

## 9. Documentation and operations

- Trace matrix and exact result: `research/windows-native-sandbox/c028-launch-trace.md`.
- VM-only comparison and operator gates: `research/windows-native-sandbox/adr-0021-authority-options.md` and C028 plan.
- Registry and roadmap now record C028's conditional negative closure; C029 remains blocked on explicit maintainer ADR-0023 disposition.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | CRT and no-CRT children both time out before user entry under `CreateProcessWithTokenW` | This runner cannot establish a usable token-child launch path or prove a restricted read boundary. | Do not admit the Windows backend. Any further launch-authority selection requires C029's reviewed architecture decision. |
| medium | Privilege-enabled `CreateProcessAsUserW` comparison and child desktop/profile/environment state remain unobserved | Hosted evidence cannot distinguish the loader substage or generalize to Windows 11 or a consented VM. | Run only on a separately consented disposable Windows VM under a newly authorized bounded probe. |

## 11. Roadmap disposition

C028's hosted phase is conditionally closed with a negative pre-user-entry result. C029 may use this disposition as evidence, but remains blocked pending explicit maintainer acceptance/rejection of proposed ADR-0023. C030 and C031 remain mutually exclusive and blocked. No C025/C026, C011/M004–M006, or C015–C018 plan is unblocked.

## 12. Registry updates

- Mark C028 conditionally closed and link this record from `plans/registry.md` and the Windows native sandbox roadmap.
- Keep C029 blocked pending maintainer ADR-0023 disposition.
- Keep C030 and C031 blocked pending C029 selection and their distinct host requirements.
