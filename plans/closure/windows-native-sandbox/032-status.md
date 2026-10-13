# Windows Native Sandbox C032 — Closure Status

Status: closed (negative bounded launch feasibility)

Source implementation plan:

- `plans/implementation/windows-native-sandbox/032-two-stage-dedicated-principal-read-confinement-feasibility.md`

Source subsystem roadmap:

- `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0022-bounded-successor-feasibility`

Repository baseline reviewed: `920aa08ea484a9230e1508e9bbaad58fbaefd498`; tested source SHA: `cd4bc1e25dd858a0b53a43a9935eedfbb359031e`.

Implementation commits:

- `ff0443ca` — test-only C032 Windows two-hop runner and workflow.
- `ebe91cce`, `cd4bc1e2` — runner bootstrap correction and child-entry/Job telemetry.
- GitHub Actions run [38405423203](https://github.com/dbowm91/codegg/actions/runs/38405423203), exact tested SHA `cd4bc1e2`.

## 1. Executive finding

C032 is closed as a negative hosted launch-feasibility result. The normal disposable account read the synthetic file whose DACL grants broad `Everyone` read. Its first-hop PowerShell runner launched with the account SID, verified its owned kill-on-close Job before resume, and returned suspend count 1. The runner created a restricted token and `CreateProcessAsUserW` created the inner child suspended. The child was verified in the inherited Job before resume and returned suspend count 1, but it did not write its first script-entry marker within 30 seconds. The timeout is not a child read denial: no child token context, host read, alias, stage, shell, Git, handle, or IPC result was obtained.

This Windows Server 2025 hosted diagnostic does not qualify the two-stage topology for production or general Windows 11 support. Windows constrained execution remains unavailable and fail-closed.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Normal dedicated account reads the unchanged broad-readable file | Run `38405423203`, `C032_REPORT.normal_account_same_file_read` | pass | First-hop account SID `S-1-5-21-1643835476-1616584234-1346609752-1003`; the same synthetic file was used for any restricted-child probe. |
| Trusted first hop runs as the new standard account | `C032_REPORT.runner_sid`, immutable runner path/hash | pass | `pwsh.exe` at `C:\Program Files\PowerShell\7\pwsh.EXE`, SHA-256 `BFB46AF89433268872DDB43D1CA7A3F433452EE91ED356A9786940F90118E285`; helper script was read-only to the account and hash `A994BE97BFA4109BD3B01C75DA2B780A009243EE8037117A8857DE15D11F9C22`. |
| First hop is Job-owned before resume | Same report | pass | `first_hop_owned_job_verified_before_resume=true`; previous suspend count was 1. |
| Runner creates its own restricted token and an inner child | Same report | partial | Restricted-token creation and `CreateProcessAsUserW` succeeded; inner child was created suspended, Job membership was verified, and prior suspend count was 1. The child did not reach its script-entry marker. |
| Actual child token and same-file read restriction | Entry marker absent after 30-second wait | not measured / negative startup | No actual child token groups/restricted SID or file-access attempt was observed. Do not count timeout as denial. |
| Stage, PowerShell/cmd, Git, alias, handle, IPC, descendant probes | Entry marker absent | not run | No result from the child exists. |
| Fixture integrity and cleanup | Same report and `C032_CLEANUP` | pass | Host and secret DACLs, secret bytes, and stage DACL were unchanged; local account, profile, and owned fixtures were removed. |

## 3. Production implementation evidence

The experiment is limited to `scripts/windows_two_hop_feasibility.ps1` and `.github/workflows/windows-two-hop-feasibility.yml`. It uses one random disposable local account, a fixed read-only first-hop script, a narrow environment block, an owned Job, synthetic fixtures, and a caller-restricted token. No released launcher, backend registry, `SandboxRequest::Required`, installer, original workspace/profile ACL, service, account persistence, schema, or protocol changed.

## 4. Verification executed

### Commands and hosted evidence

- `rtk pwsh -NoProfile -Command '...Parser...ParseFile("scripts/windows_two_hop_feasibility.ps1"...)...'` — passed on the Linux development host.
- `rtk git diff --check` — passed.
- `rtk bash scripts/verify.sh quick` — passed on the implementation branch before the final C032 telemetry-only correction.
- GitHub Actions run `38405423203`, Windows Server 2025 Datacenter build `26100`, exact source SHA `cd4bc1e25dd858a0b53a43a9935eedfbb359031e` — workflow completed successfully and collected a negative result. It is not a strict-positive result.

### Result details

`C032_REPORT` recorded `inner_child_created=true`, `inner_child_suspend_count=1`, `inner_child_owned_job_verified_before_resume=true`, `inner_child_resume_succeeded=true`, `child_entry_marker_seen=false`, and `launch_failure="Restricted child timed out (wait=258)."`. `C032_CLEANUP` recorded removal of the account, profile, and fixtures. No Windows 11 or separately controlled VM result was obtained.

## 5. Invariant review

- The normal and restricted identities were separate; the test restricted only the new account's own primary token.
- The limiting SID was granted only to the new synthetic stage and fixed runner path needed for the experiment.
- The first-hop and inner child were suspended until Job membership was checked; both observed resume counts were 1.
- No child-access denial, network isolation, or production capability is claimed.
- Existing Windows `Required` requests remain unavailable and fail-closed.

## 6. Failure and recovery review

The inner child wait was bounded to 30 seconds. After timeout, the first-hop runner wrote a failure report; the outer controller closed the kill-on-close Job, removed only the random account/profile it created, and deleted its unique fixture root. The report did not include a child exit code or entry state beyond the absent marker. Reboot, crash recovery, concurrency, credential rotation, and independently controlled VM behavior remain unqualified.

## 7. Migration and compatibility review

No product API, schema, dependency, MSRV, configuration, installer, or legacy execution behavior changed. The probe used the hosted Windows Server 2025 image only; this does not establish Windows 11 support.

## 8. Security review

The unconfined dedicated account read the broad-readable synthetic fixture, as intended for the positive control. The restricted child never reached the first instruction marker, so no read-boundary claim follows. The fixed runner and synthetic ACLs were checked, and the experiment made no original-tree ACL change. The hosted runner result is bounded diagnostic evidence only.

## 9. Documentation and operations

- Added the test-only driver and isolated workflow.
- Updated C032 plan, Windows native sandbox roadmap, registry, and C034 comparison research.
- C032's report distinguishes process creation/resume from actual child entry and access evidence.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | The restricted child did not reach its first-instruction marker on the hosted Windows Server 2025 runner. | No usable tool process or restricted-read boundary was established. | Do not select the two-stage route. Reconsider only in a newly authorized, bounded plan with an eligible controlled Windows host and actual child-entry evidence. |
| medium | Child token details, aliases, inherited handles, IPC, tool compatibility, and Windows 11 behavior were not observed. | The hosted result cannot establish the broader threat boundary or platform support. | Keep the route unqualified; do not infer denial from the timeout. |

## 11. Roadmap disposition

C032 is closed as a negative bounded launch-feasibility experiment. C034's written comparison dependency is satisfied. No C011/M004–M006, C015–C018, C030, or C031 work is unblocked; the production Windows constrained-execution state remains unavailable.

## 12. Registry updates

- Mark C032 closed (negative bounded launch feasibility) and link this record from the registry and subsystem roadmap.
- Record that C034 may proceed to a no-admission comparison using C032's written negative disposition.
- Keep all production Windows sandbox and terminally blocked ADR-0023 routes unavailable.
