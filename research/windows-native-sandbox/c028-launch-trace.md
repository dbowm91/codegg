# C028 Token Launch Initialization Trace

Status: negative bounded hosted phase; privileged comparison VM-gated

## Evidence

- GitHub Actions run: `38396698509`.
- Tested branch SHA: `33c82f56039df173fe04f8aabc1a863fa225f006`.
- Runner: Windows Server 2025 Datacenter, version `10.0.26100`, build `26100`, UBR `33438`.
- Runner identity: disposable GitHub hosted `runneradmin`; caller session ID 2.
- Disposable child SID: `S-1-5-21-1643835476-1616584234-1346609752-1003`.

| Case | `LOGON_WITH_PROFILE` | Child process/token session | `ResumeThread` previous count | First user entry | Result |
|---|---:|---:|---:|---|---|
| CRT, no profile | 0 | 2 / 2 | 1 | no marker | 15-second pre-user-entry timeout |
| CRT, profile | 1 | 2 / 2 | 1 | no marker | 15-second pre-user-entry timeout |
| no CRT, no profile | 0 | 2 / 2 | 1 | no marker | 15-second pre-user-entry timeout |
| no CRT, profile | 1 | 2 / 2 | 1 | no marker | 15-second pre-user-entry timeout |

For every case, `CreateProcessWithTokenW` created a primary-token process, the child's SID matched the disposable account, and the process was verified in its kill-on-close Job before resume. The report collection completed and the job removed the disposable principal and unique fixture root. C027's existing hosted `CreateProcessAsUserW` control returned 1314; its unrestricted `CreateProcessWithTokenW` child also timed out. C027's ordinary-account baseline wrote its probe report, but its cmd and Git smoke checks returned Win32 21, so that baseline is not tool-compatibility evidence.

## Interpretation

The timeout occurs before either the CRT `main` marker or a custom no-CRT entry marker. Turning profile loading on did not change the result. This isolates the observed wait to pre-user-entry process initialization for this API and runner. It does not identify a specific loader substage, prove a desktop/session defect, or generalize beyond this hosted Windows Server 2025 image. Because the child never reached user code, the probe did not observe HKU profile state, desktop name, environment values, console state, or result-file behavior.

The test held the default desktop, target-token environment block, and unredirected stdio constant. The plan's privileged `CreateProcessAsUserW` comparison was not run: a GitHub hosted runner is not the separately consented disposable VM required by C028. Do not enable privileges on the shared runner or infer read denial from this trace.

## Disposition

C028 is conditionally closed as negative bounded feasibility. Windows constrained execution remains unavailable. C029 needs an explicit maintainer ADR-0023 decision before either mutually exclusive C030 broker or C031 guest feasibility plan can start.
