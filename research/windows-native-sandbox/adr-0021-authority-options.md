# ADR-0021 research: Win32 launch authority or native Windows guest

Status: evidence-based comparison, not production qualification
Baseline: `7a27bc7e6566616fc1b2b8a69637f12050d42843`; 2026-10-13.

## Current native evidence

C023 mandatory Windows process-launcher CI selected 48 passing tests (run `38359288381`). C027 run `38369937432` corrected Win32 Unicode interop; normal account positive broad-file read, private-file denial and stage write passed. The unrestricted `CreateProcessAsUserW` test returned 1314; `CreateProcessWithTokenW` child remained incomplete beyond 15 seconds. No restricted-token host read-denial exists. C025 was never executed, C026 recorded no admission. C011/C013 demonstrated root ACL leases on original populated host files unsafe; C014 AppContainer Git minimum failed; C020 MXC incomplete.

## Discriminating next native trace

Record `ResumeThread` **previous suspend count** (not only failure), first-instruction native marker before JSON output, actual process/thread state, loader/DLL, user-profile HKU, session, desktop/window-station, target environment and pipe/console accessibility. A first-instruction marker with missing JSON indicates a different failure from a still-suspended child. Run fixed baseline alongside bounded one-at-a-time variants (`LOGON_WITH_PROFILE`, desktop context, env/stdio, minimal no-CRT helper). Do not randomly adjust flags. `CreateProcessAsUserW` scoped privilege comparison requires isolated consented VM because shared Windows runner lacks privileges; do not grant host-wide user rights.

## Options matrix

| Property | Opt-in privileged broker | Separate Windows guest | Fail-closed default |
|---|---|---|---|
| Execution | True restricted user token if proven | Guest-native Win32 tools | unavailable |
| Host read boundary | Must prove restricting SID actually denies broad-readable files | Host kernel separated, no mapped secret/workspace/IPC | no constrained process |
| Host trust | Privileged SCM + authenticated IPC + profile/account lifecycle | Hypervisor + guest image + file-transfer bridge | unchanged |
| Compatibility | Limited by token and developer tools | Guest setup/edition/availability | unchanged |
| Concurrency | Per-run Jobs/account ownership | Windows Sandbox one instance; Hyper-V separate pool | N/A |
| Network | Unclaimed until independently qualified | Sandbox enabled by default; explicitly disable and test | Unclaimed |
| User privilege | Explicit service install/consent | Hypervisor/feature provisioning/consent | None |

A broker pipe with default DACL can grant overly broad access. The privileged server must **check** `ImpersonateNamedPipeClient` and abort if it fails; caller-supplied PID/name is not authentication. Broker cannot be an elevated shell or take raw agent-generated arbitrary launch parameters. Windows Sandbox is unavailable on Windows Home; mappings/clipboard/network are security-sensitive. WSL2 is a Linux execution profile, not Windows native toolchain parity.

## Decision rule

Only C028 is dependency-ready. C029 requires exact observed C028 disposition and maintainer architecture approval. C030/C031 blocked and mutually exclusive until selected. Even a good result cannot mark filesystem/network `Enforced` or unblock C011/M004 without a later independently reviewed production security path.

Microsoft references: https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-createprocesswithtokenw ; https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights ; https://learn.microsoft.com/en-us/windows/security/application-security/application-isolation/windows-sandbox/ .
