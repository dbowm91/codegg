# ADR-0021 — Windows Privileged Launch versus Guest Isolation

Status: proposed — no production route selected
Date: 2026-10-13
Decision owners: Codegg maintainers
Evidence baseline: `7a27bc7e6566616fc1b2b8a69637f12050d42843`

References: `plans/000-long-term-specification.md` sections 4, 17, 27, 29; `plans/001-terminology-and-domain-model.md` section 8; `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`; `plans/adrs/ADR-0016-cross-platform-sandbox-launch-authority.md`; `plans/adrs/ADR-0020-windows-native-sandbox-after-appcontainer-feasibility.md`; `plans/subsystems/windows-native-sandbox-roadmap.md`.
Research: `research/windows-native-sandbox/adr-0021-authority-options.md`.

## Context

C027 (`plans/closure/windows-native-sandbox/027-status.md`; Windows Server 2025, run `38369937432`, SHA `eb1509b9`) corrected Unicode interop and proved the ordinary dedicated account could read the synthetic broad-group-readable host file and write staging data. `CreateProcessAsUserW` failed 1314 (`ERROR_PRIVILEGE_NOT_HELD`); unrestricted `CreateProcessWithTokenW` created a suspended Job-owned child but timed out after 15 seconds. **No restricting-SID child read occurred**. C011/C013 reject mutations of populated original-host ACLs; C014 and C020 had qualified-negative/incomplete compatibility findings. Windows `SandboxRequest::Required` remains unavailable.

C027's harness did not record `ResumeThread`'s returned previous suspension count, and a JSON-output timeout does not prove that a helper reached its first instruction. `CreateProcessWithTokenW` uses the caller's session and does not automatically load the target user's registry profile. These are diagnostic hypotheses, not evidence that the native route is qualified.

## Decision drivers

Required: real denial of original host files including broadly readable data; useful native Git/shell; canonical `ManagedProcessService` ownership, process/Job/suspension safety; no arbitrary elevated tool execution; no original-host ACL modifications; bounded lifecycle/recovery; explicit operator consent and OS support; independently reviewed negative tests.

## Alternatives

**A — Retain unavailable:** safe default, no new host privilege, no native constrained tool execution.

**B — Explicitly consented privileged launcher broker:** narrowly trusted SCM service with only needed privileges; fixed signed helper/typed authorized launch manifest, no arbitrary executable/command/token/SID/ACL operations; restrictive named pipe DACL, actual client-token authentication/checked impersonation, nonce and replay prevention, no agent/TUI direct endpoint. Child always unprivileged and must **actually** fail a broad-group-readable host-file read under a verified restricting-SID token. A privileged service alone proves nothing about filesystem isolation. High attack surface: confused deputy, persistent service/accounts, credential custody, restart and pipe escape.

**C — Windows guest (Windows Sandbox/Hyper-V):** can provide separate Windows kernel boundary but guest file transfer/host bridges require independent audit. Windows Sandbox is not supported on Home, currently allows a single instance, and enables networking and clipboard by default. Do not map original user workspace or secrets into guest. Hyper-V guests require separately qualified hypervisor/edition, image provenance, resources and concurrency. WSL2 is not native Win32 sandbox parity.

**D — Reuse existing root DACL or AppContainer ACL workaround:** rejected by C011/C013 host-data leakage and descriptor drift; do not reopen without new security decision.

## Proposed decision and gates

1. **C028** records a bounded native phase diagnosis of the C027 timeout, plus controlled privilege comparison **only on operator-consented isolated Windows VM**.
2. **C029**, after C028 closure, presents explicit security/cost/host-compatibility evidence for exactly one disposition: broker feasibility (C030), native guest feasibility (C031), or retain unavailable. This ADR is **not accepted** until a maintainer explicitly reviews and accepts that selection. Missing VM evidence cannot be treated as a broker success.
3. C030 and C031 are mutually exclusive blocked experiments; the selected one alone may become ready after ADR acceptance and its operational host gate. A completed experiment does not grant production admission.
4. C011/M004–M006 and C015–C018 remain blocked. No automatic privileged install, no backend registration, no network or filesystem `Enforced`, no FullHost fallback, no historic closure rewrite.

## Consequences and migration

Broker path adds Windows service privilege and untrusted IPC authorization surface; guest path adds virtualization/edition, single-instance or VM pooling, data-copy and output-promotion complexity. The current `Required` fail-closed behavior remains mandatory. No production binary, protocol, schema, user setting, installer, account or ACL change authorized by this **proposed** ADR.

## Supporting authoritative references

- https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-createprocesswithtokenw
- https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessasuserw
- https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights
- https://learn.microsoft.com/en-us/windows/win32/api/namedpipeapi/nf-namedpipeapi-impersonatenamedpipeclient
- https://learn.microsoft.com/en-us/windows/security/application-security/application-isolation/windows-sandbox/
- https://learn.microsoft.com/en-us/windows/security/application-security/application-isolation/windows-sandbox/windows-sandbox-configure-using-wsb-file
