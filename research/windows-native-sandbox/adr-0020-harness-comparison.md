# ADR-0020 research: Windows coding-harness isolation and native MXC successor

Date: 2026-10-12
Status: research and implementation-planning input (not qualified Codegg security evidence).
Codegg reference: `b1024ba719c627f8a294cb50170d48ec08ce62f4`.

## Current Codegg evidence, not to be reinterpreted

- C011: real restricted Windows child read synthetic credential directly and through junction/hardlink; original deny-path guarantee failed.
- C013: populated host-tree root ACL lease mutated a hardlink-shared file object and left security-descriptor control-bit drift after restore; grant-only allowed read also failed. Do not revive populated original-workspace ACL leasing.
- C014: Windows Server 2025 hosted run 38316065754 launched a test AppContainer child and read/wrote its own projection but Git init failed opening `/dev/null`. Subsequent denied-host, IPC, handle, Cargo and Python tests **did not run**. Investigate device `\Device\Null` as a hypothesis, not a proven cause. Original AppContainer/projection C015–C018 remains blocked.
- Existing `src/managed_process/windows.rs` implements test-only restricted child launch/Job and `src/managed_process/windows_acl_lease.rs` holds **disposable-fixture-only** ACL lease machinery. `src/security/sandbox/policy.rs` is policy/preflight, not a Windows read-denial enforcement backend.

## ADR-0020 successor hosted evidence

- **C019 accepted:** exact source SHA `2a39eb6eba4234ff865e8a92282797203dab681f`, GitHub Actions run `38330137971` (Windows Server 2025 Datacenter, `10.0.26100`, UBR `33438`, Rust/Cargo 1.89.0, Git 2.55.0.windows.5, Python 3.12.10). The 1-test actual-child matrix and 6-test restricted-process lifecycle suite passed. The per-child AppContainer-private stage was writable; direct, junction, case-alias, extended-path, hardlink, ADS, sibling, outside-write, and pipe attempts were separately denied. Host/sibling DACL snapshots and synthetic secret bytes were unchanged. Git/Python/Cargo child tool actions returned `failed`; they remain compatibility observations, not denial results. UNC, hostile inherited handle, and durable restart probes were explicitly not run with reasons in C019 output.
- **C014 device cause remains unconfirmed:** the C019 restricted and unrestricted children both reported `.NET File.Open("\\\\.\\NUL")` as `MethodInvocationException`. The probe did not capture an underlying Win32 error or identify Git/MSYS's `/dev/null` syscall, so it does not establish an ACL or device-object defect.
- **C020 host prerequisite discovered:** hosted run `38330611716` on `windows-11-arm` observed Windows 11 Enterprise 25H2, build `26200`, UBR `9457`, ARM64, installed hotfixes including `KB5126052`, loadable `C:\Windows\System32\processmodel.dll`, and the `CreateProcessSecurityEnvironment` export. This is host eligibility evidence only; no MXC policy or containment claim follows from it. The pinned v1.0.0 SDK still requires actual backend/tier and running-child policy qualification.
- **C021 first hosted attempt:** run `38330611632` created and removed its disposable local principal, but a null cleanup-path bug replaced the primary script failure. It is not candidate evidence; the cleanup/report harness is being corrected and will be rerun.

## Public implementations to learn from

| Harness | Published approach | Limit relevant to Codegg |
|---|---|---|
| OpenAI Codex | Separate local online/offline sandbox users, admin setup distinct from command runner, restricted `WRITE_RESTRICTED` tokens, SID-bound firewall | Write restrictions and dedicated identity do **not** imply blanket host-read denial; user-readable credentials / broad grants are a risk |
| Anthropic sandbox-runtime (Windows alpha) | `srt-sandbox` user, DPAPI-managed credentials, SID-bound WFP egress filters, per-session ACE grants/denies and cleanup | Still modifies host ACLs; hardlinks, descriptor restoration, app/toolchain paths and recovery require Codegg-specific evidence; not proof native Claude Code shell sandbox is enabled |
| Google Gemini CLI | Low-integrity restricted-token helper, Job, file ACL and integrity labeling; network bandwidth-rate setting | Integrity labels are persistent host mutations, Low IL protects writes rather than arbitrary reads, rate limiting is not offline isolation |
| VS Code / Copilot Agent Host | Microsoft MXC for Windows agent commands, installed serviced Windows 11 prerequisites | Experimental host/feature matrix, native API versus fallback must be measured; shell sandbox does not cover all agent-side file tools |
| Cursor | WSL2/Linux sandbox for Windows workflows | Native Windows binaries and Windows IPC are different; WSL2 does not substitute for native Windows containment |
| OpenCode | Model/tool permission policy | Shell approval alone is not OS-level containment |

References:
- Codex: https://openai.com/index/building-codex-windows-sandbox/
- Anthropic: https://github.com/anthropics/sandbox-runtime/blob/main/README.md
- Gemini: https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/sandbox/windows/GeminiSandbox.cs
- VS Code: https://code.visualstudio.com/docs/agents/run/agent-sandboxing and https://github.com/microsoft/vscode/blob/main/src/vs/platform/sandbox/common/terminalSandboxMxcRuntime.ts
- MXC: https://github.com/microsoft/mxc/releases/tag/v1.0.0 ; https://github.com/microsoft/mxc/blob/v1.0.0/docs/backends/process-container/os-version-support.md
- MXC host setup: https://github.com/microsoft/mxc/blob/v1.0.0/docs/host-prep.md

## Time-sensitive MXC correction to M007

MXC published SDK v1.0.0 on 2026-10-07; tag `v1.0.0` resolves to annotated tag `d119563ff7fa77c990c1b3762693d03dc763b605` and commit `7bf210247986cb73b1b314df60c2f1109c479c0b`. The documented stable Rust SDK entrypoint is `mxc_sdk::v1`, and public discoverable execution/lifecycle APIs have changed from the older preview. That release does **not** establish Codegg safety. The pinned MXC repository toolchain remains Rust 1.93, differing from Codegg MSRV 1.89; an isolated experiment toolchain/binary runner is preferable to a workspace-wide MSRV bump.

MXC release notes describe process isolation on serviced Windows 11 24H2/25H2 and version-specific OS updates. The current support table gives Windows 11 process-isolation build floors (at time consulted) 24H2 `26100.9278`, 25H2 `26200.9278`, 26H2 `26300.9550` and 26H1 `28000.2804`. VS Code Agent Host separately documents September 2026 security updates, including KB5124008 / KB5124012, for its host. **Neither an edition label nor a base build (e.g. Windows Server 2025 `26100`) proves native Windows 11 PSEC availability.** Probe live OS features, installed servicing, effective MXC backend/tier and policy-honor result. If fallback is AppContainer+DACL or demands persistent host-wide ACE/device security changes, treat as nonqualifying for the no-source-mutation candidate. MXC README continues to warn early-preview generated profiles may be overly permissive; stable SDK ABI does not establish hardened policies.

The predecessor `architecture/windows-mxc-feasibility.md` and `plans/closure/windows-native-sandbox/007-status.md` are historically correct for their *older pinned revision* and unexecuted Windows 11 matrix. C020 is a new, independently bounded v1 re-assessment, not a fabricated reversal of M007 closure.

## Comparison dimensions and decision thresholds

Both candidate backends must use **identical synthetic** source/staging/sibling/secret/link/ADS/IPC/handle/descendant probes owned by C019. Classify per dimension: allowed read, denied read, allowed write, denied write, process/descendant supervision, network *unclaimed*, identity/ACL alteration, tool compatibility, setup/rollback, supported builds/editions, and latency. Distinguish restrictions obtained from kernel checks from best-effort string admission, path copying, or approval. Test the negative direct and alias paths **even if a smoke command fails**; a failed Git process is not a reason to skip the host-denial matrix.

- **MXC positive feasibility:** confirmed native supported Windows 11 backend and actual tool plus negative resource matrix, no forbidden persistent host changes, bounded invocation, exact SDK/OS proof.
- **Dedicated-user positive feasibility:** real isolated principal and Job, useful Git/shell subset with demonstrable original-host credential/alias/IPC denial, no broad original profile grants, fixtures and reversible provisioning verified.
- **Negative feasibility:** minimum tooling cannot work safely, or any claimed prohibited access succeeds, or mechanism requires disallowed permanent host mutations.
- **Blocked:** requisite VM/Windows edition/OS capability or administrator consent unavailable; never substitute Windows Server 2025 hosted GitHub actions for a serviced Windows 11 test host.
- **No automatic winner:** C022 makes a documented comparison/maintainer gate. Neither candidate may modify production admissions, C015–C018 or M004/M005 status before a separately approved integration decision.

## Scope boundaries

No Windows kernel driver, filter driver, permanent workstation ACL modification, global `\Device\Null` descriptor change, automatic UAC from an agent-run tool, credentials in source, cross-agent authorization bypass, new public protocol or default dependency on MXC is authorized by this research. A separate stronger VM/WSL execution target remains a deferred third option if neither native candidate passes.
