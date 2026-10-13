# C034 — Independent Successor Comparison and Capability Decision

Status: **closed — no admissible production candidate**
Date: 2026-10-13
Authority: ADR-0022 test-only successor feasibility; ADR-0021 no-admission remains unchanged.

## Evidence identity

| Candidate | Exact source SHA / hosted run | Host and result |
|---|---|---|
| C032 two-stage dedicated principal | SHA `cd4bc1e25dd858a0b53a43a9935eedfbb359031e`; [run 38405423203](https://github.com/dbowm91/codegg/actions/runs/38405423203) | Windows Server 2025 Datacenter build 26100. First-hop normal account read the broad-readable fixture. The runner created the restricted token and inner child, verified Job membership before resume, and observed suspend count 1; no child-entry marker arrived within 30 seconds. |
| C033 MXC v1.0.0 | SHA `ff0443ca33a14282dc81fdffd35240265151549f`; [run 38404451372](https://github.com/dbowm91/codegg/actions/runs/38404451372) | Windows 11 Enterprise 25H2 ARM64 build 26200, UBR 9457. BaseContainer block-mode capture recorded 13 non-truncated events; tested reads/aliases and writes were denied. Native helper initialization failed with `STATUS_DLL_INIT_FAILED`; inherited handle was unavailable. |

The hosted workflow conclusions mean the probes reported and cleaned their synthetic fixtures. They do not mean either candidate passed strict-positive acceptance.

## Side-by-side capability disposition

| Dimension | C032 two-stage runner | C033 MXC v1.0.0 | Decision |
|---|---|---|---|
| Filesystem read | Normal dedicated account read the same broad-readable fixture. Restricted child never reached entry, so no restricted read result. | Normal child read the same fixture; MXC child denied direct, junction, case, hardlink, ADS, extended-path, and sibling reads. | C033 has bounded deny evidence; neither establishes a complete production read allowlist. |
| Filesystem write | Restricted child did not reach entry. | Outside write attempt was denied. Stage write was attempted but not independently asserted. | Insufficient to claim complete write containment or promotion semantics. |
| Credentials / IPC / handles | Child probes did not run. | Synthetic named pipe denied; native inherited-handle helper failed initialization, so handle transfer was not measured. | Required handle/credential boundary remains unqualified. |
| Process tree | First-hop and inner child were verified in the owned Job before resume; inner child did not reach user code. | Main descendant was gone when MXC returned; helper process failed initialization. | Bounded cleanup observations only; cancellation, restart, and concurrency are unqualified. |
| Tool compatibility | No child entry, so PowerShell/cmd/Git did not run under the restricted token. | Git init/status, cmd, Python script, and offline Cargo smoke reported success; native helper failed. | MXC misses the required native helper/handle gate. |
| Network | Not tested. | Not configured or tested. | Neither candidate has a network claim. |
| Host metadata / ACLs | Synthetic host/secret/stage DACLs and secret hash unchanged; owned account/profile/fixtures removed. | Synthetic host/secret DACLs and secret content unchanged. | No original host ACL mutation was observed. |
| Lifecycle / operations | One ephemeral hosted account only; no reboot, crash, or concurrency. | One hosted process tree; no independent cancellation/restart/concurrency. | Not production recovery qualification. |

## Threat review and disposition

C032 removed the specific cross-account token transition that blocked C024/C027, but its inner restricted process did not reach the first user instruction on the hosted Windows Server 2025 image. The timeout is not evidence of denied reads. No shell/tool compatibility or child token attestation exists.

C033 provided stronger bounded evidence on a serviced Windows 11 ARM host: BaseContainer selected, direct and alias reads denied against the same broad-readable synthetic object, and block-mode capture produced a complete 13-event summary. Its native helper failed before an inherited-handle result, and the child SID/group observation and independent stage-write assertion were absent. The experiment also did not test network, restart, or concurrency. These gaps are sufficient to reject production admission under ADR-0022's explicit gates without treating any missing operation as denied.

The executable production route remains **none**. Retain fail-closed Windows constrained execution: `SandboxRequest::Required` remains unavailable, no filesystem or network `Enforced` value is emitted, and there is no unrestricted fallback. No write-only security profile is introduced. Choosing one would require a separate explicit policy/approval decision.

No independent positive security review was requested or claimed because neither candidate was strict-positive and no production proposal is made. There is no follow-on implementation in this line. C030/C031 remain terminally blocked under the rejected ADR-0021 decision; C011/M004–M006 and C015–C018 remain blocked.

## Operations and future reconsideration gates

Any future proposal requires a new explicit architecture decision and bounded plan. At minimum it must establish actual child entry and token identity, the same-file broad-read positive/negative contrast, stage and minimum-tool behavior, alias/IPC/inherited-handle results, Job/descendant cleanup, and unchanged original metadata. It must then separately qualify cancellation, restart/reboot, concurrency, host-version support, account/helper lifecycle, and network policy before making claims in those dimensions. The current evidence does not satisfy those gates.
