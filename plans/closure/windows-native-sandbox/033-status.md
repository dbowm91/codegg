# Windows Native Sandbox C033 — Closure Status

Status: closed (negative bounded candidate feasibility)

Source implementation plan:

- `plans/implementation/windows-native-sandbox/033-mxc-denial-telemetry-and-toolchain-requalification.md`

Source subsystem roadmap:

- `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0022-bounded-successor-feasibility`

Repository baseline reviewed: `920aa08ea484a9230e1508e9bbaad58fbaefd498`; tested source SHA: `ff0443ca33a14282dc81fdffd35240265151549f`.

Implementation commits:

- `ff0443ca` — isolated MXC v1.0.0 C033 harness and Windows 11 ARM workflow.
- GitHub Actions run [38404451372](https://github.com/dbowm91/codegg/actions/runs/38404451372), exact tested SHA `ff0443ca`.

## 1. Executive finding

C033 is closed as a negative bounded candidate result. On Windows 11 Enterprise 25H2 ARM64 (build `26200`, UBR `9457`), the pinned MXC v1.0.0 ProcessContainer selected `base-container` with `CaptureDenials` and `FilesystemDeniedPaths`. The normal child read the synthetic broad-readable file; the MXC child denied its direct and exercised alias reads, outside write, and named-pipe access. Git init/status, a real Python descendant script, and an offline Cargo smoke completed. Block-mode denial capture produced 13 events with `deniedResourcesTruncated=false`; the original fixture DACL/content stayed unchanged and the descendant was gone at return.

The candidate did not meet the required full compatibility/probe gate: the native helper failed to initialize with `-1073741502` (`STATUS_DLL_INIT_FAILED`), so inherited-handle denial was unavailable. This is not a strict-positive candidate and does not authorize production use. Network behavior is untested.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Eligible host and pinned runtime | Run `38404451372`, `C033_HOST`, `C033_REPORT` | pass | Windows 11 Enterprise 25H2 ARM64, build `26200`, UBR `9457`; MXC SDK `1.0.0`, isolated toolchain `1.93.1`. |
| Selected tier and requested capture mode | `C033_MXC_BACKENDS`, `C033_MXC_PROBE`, `C033_REPORT` | pass | `processcontainer`, `base-container`, `CaptureDenials`, and `FilesystemDeniedPaths`; capture mode was `block`, warnings empty, execution outcome `Exited(0)`. |
| Same-file broad-read positive and denied reads | `same_file_normal_child_baseline=allowed`; child stdout in `C033_REPORT` | pass for exercised fixture | The normal PowerShell child read the same `Everyone`/`Users`/`Authenticated Users` readable synthetic secret. Direct, junction, case, hardlink, ADS, extended-path, and sibling reads were denied by the MXC child. UNC was not run. |
| Stage, Git/shell, Python, Cargo compatibility | Child stdout | pass/partial | Git init/status, cmd shell, Python script/descendant probe, and `cargo check --offline` reported success. The harness attempted a stage write but did not independently assert the resulting file, so stage-write evidence is partial. |
| Native helper and inherited handle | Helper execution and `inherited_handle_probe_execution` | fail / unavailable | Helper exited `-1073741502` (`STATUS_DLL_INIT_FAILED`); the handle result is `inherited_handle=unavailable`, not denied. |
| Block-mode deny telemetry | `C033_REPORT.denial_capture` | pass | JSON document present; 13 events: 8 `file:read`, 3 `file:write`, 1 `other:read`, 1 `ui:unknown`; no resource truncation. Raw resource paths were not copied into the summary report. |
| Host mutation and descendant cleanup | `C033_REPORT` | pass for exercised scope | Fixture DACLs and secret content unchanged; descendant was no longer alive when the invocation returned. Independent cancellation/restart/concurrency were not run. |
| Network, UNC, and full child identity | Report unsupported fields and stdout | not run / not observed | Network is not claimed. `whoami` output did not appear in the captured stdout, so a child SID/group attestation is not claimed. |

## 3. Production implementation evidence

All changes are confined to `research/windows-native-sandbox/mxc-c033-harness/` and `.github/workflows/windows-mxc-c033-requalification.yml`. The harness pins MXC v1.0.0 and requests block-mode denial capture with a synthetic fixture. No MXC dependency entered the CodeGG workspace or shipped binary. No production backend, account, firewall rule, original host ACL, schema, installer, or user-visible guarantee changed.

## 4. Verification executed

### Commands and hosted evidence

- `rtk cargo fmt --manifest-path research/windows-native-sandbox/mxc-c033-harness/Cargo.toml -- --check` — passed after formatting.
- `rtk cargo check --locked --manifest-path research/windows-native-sandbox/mxc-c033-harness/Cargo.toml` — passed locally.
- `rtk bash scripts/verify.sh quick` — passed at implementation SHA `ff0443ca`.
- GitHub Actions run `38404451372`, Windows 11 ARM64 exact tested SHA `ff0443ca` — workflow passed as an evidence-collection job. Its C033 strict lifecycle gate explicitly reported the unavailable inherited handle and failed helper initialization; workflow success is not a strict-positive result.

### Native report

`C033_REPORT` recorded `result="Exited(0)"`, `same_file_normal_child_baseline="allowed"`, `denial_capture.document_present=true`, summary count 13 with no truncation, unchanged fixture DACLs/content, `inherited_handle_probe_execution="Exited(-1073741502)"`, and `descendant_alive_at_return=false`. Child stdout recorded all exercised reads and outside write denied, Git init/status allowed, Python script exit 0, and offline Cargo smoke allowed.

## 5. Invariant review

- The experiment used synthetic files and never changed an original host tree's ACL or contents.
- Deny capture used `block`, not permissive `allow` audit mode.
- An unavailable inherited-handle result is not counted as denial.
- Filesystem observations do not imply network enforcement or production admission.
- Windows `SandboxRequest::Required` remains unavailable and fail-closed.

## 6. Failure and recovery review

The main MXC child completed and its descendant was gone at return. The separately started native handle probe failed during process initialization; its outcome is recorded as unavailable. The harness removed its unique synthetic fixture. Independent timeout/cancellation, crash, restart, concurrency, Windows Server, and user-host cleanup/recovery were not qualified.

## 7. Migration and compatibility review

No product schema, public protocol, configuration, installer, dependency, or MSRV changed. MXC's Rust toolchain `1.93.1` stayed isolated from the CodeGG workspace MSRV.

## 8. Security review

The run provides observed negative access results for the tested synthetic read/alias/write/pipe operations under one supported BaseContainer host and block-mode capture. It does not qualify inherited-handle protection, native helper compatibility, all child token identity, network policy, cancellation/restart, or the complete threat boundary. No security admission follows.

## 9. Documentation and operations

- Added an isolated C033 harness and a separate Windows 11 ARM workflow.
- The workflow records OS edition/build/UBR/architecture and the report records the selected MXC backend, capture mode, tool results, synthetic metadata, descendant result, and unsupported probes.
- Updated C033 plan, roadmap, registry, and C034 comparison research.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Native helper initialization failed and inherited-handle denial was not measured. | Required tool/handle qualification is incomplete; candidate cannot pass the strict-positive gate. | Do not select MXC for production. Reconsider only under a new bounded plan that records actual helper entry and handle results. |
| medium | Stage-write result was attempted but not independently asserted; child identity and token group output were not observed. | These details cannot be treated as verified. | Keep the result scoped to reported process outcomes and deny telemetry. |
| low | Network, UNC, cancellation, restart, and concurrency were not tested. | Network and lifecycle guarantees remain unknown. | Do not claim these dimensions. |

## 11. Roadmap disposition

C033 is closed as a negative bounded candidate result. Its written disposition satisfies C034's dependency. It does not unblock C011/M004–M006 or C015–C018 and does not change historical C020.

## 12. Registry updates

- Mark C033 closed (negative bounded candidate feasibility) and link this record from the registry and subsystem roadmap.
- Record the 13-event block-mode report and unavailable native-handle probe in C034's comparison.
- Keep production Windows sandbox and network enforcement unavailable.
