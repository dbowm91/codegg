# C007 M004/M005 Baseline and Denial Fixture Matrix

Date: 2026-10-14

This is the bounded fixture/design record for
`plans/implementation/linux-execution-isolation/015-m004-m005-process-ipc-offline-fixture-and-denial-matrix.md`.
It separates effects observed under today's Landlock-only path from future
M004/M005 requirements. It does not claim enhanced isolation.

## Host and legacy baseline

Host: Ubuntu 24.04.5 LTS, x86_64, Linux `6.8.0-146-generic`. The active LSM
list includes Landlock and Yama. AppArmor is enabled with the current profile
`unconfined`; SELinux is absent. `/proc/self/status` reports `Seccomp: 0`.

`tests/sandbox_containment.rs::legacy_landlock_leaves_host_process_tcp_and_abstract_uds_reachable`
uses only owned fixtures and the production sandbox helper. A same-UID child
under Landlock can see and signal its controlled host PID, connect to a host
loopback TCP listener, and connect to a host abstract Unix-domain socket.
The synthetic `/proc/<host-pid>/environ` read is denied by the host's ptrace/Yama
policy; that denial is not credited to Landlock. The helper status identifies
Landlock and includes the explicit `no network isolation` limitation. These
observations establish that Landlock does not supply the M004/M005 process,
abstract-socket, or loopback-network contract.

## Fixture matrix for later native backends

| Dimension | Safe fixture and child probe | Legacy Landlock baseline | Required enhanced evidence | Current status |
|---|---|---|---|---|
| Host PID/process visibility | Parent-owned same-UID `sleep` child; inspect `/proc/<pid>`, `kill(pid, 0)`, and signal only that fixture | PID visible and signal permitted; proc-environ is host/Yama dependent | Host PID absent/inaccessible while own descendants, forwarding, timeout, and reaping continue | Baseline process visibility observed; enhanced path blocked |
| `/proc` and namespace handles | Read synthetic process status/fd links and namespace links | Proc policy and Yama may deny selected files; record errno and attribution | Private proc view hides host process and host namespace handles | Not qualified under an enhanced child |
| Loopback TCP | Parent listener on ephemeral `127.0.0.1` port; one-byte request/ack | Connection succeeds | Offline child cannot reach host listener; own loopback, if retained, is separately scoped | Legacy connection observed; OfflineRequired unimplemented |
| Abstract AF_UNIX | Parent abstract stream socket with unique test name; one-byte request/ack | Connection succeeds | Host abstract socket is unreachable; IPC namespace alone is insufficient | Legacy connection observed; enhanced denial unimplemented |
| Pathname AF_UNIX and inherited sockets | Private temp-root socket; parent-opened connected descriptor above fd 2 | Not exercised by this baseline test | Path and abstract sockets denied; inherited descriptors absent or explicitly reported | Design only |
| UDP, IPv4/IPv6, DNS | Parent-owned loopback UDP responders and controlled local DNS fixture; no external traffic | Not exercised by this baseline test | Actual blocked connects/queries with a positive control proving responder works | Design only |
| `SSH_AUTH_SOCK`, daemon/runtime sockets | Synthetic pathname sockets and explicit env/FD sentinels | Not exercised by this baseline test | No host `/run`, agent, container, daemon or cross-session socket reachability | Design only |
| Seccomp | Tiny native child issues only selected harmless denied syscalls after a control syscall | No seccomp is applied by the legacy helper | Filter installed before target; expected denial plus ordinary toolchain controls | No production filter; M004 blocked |
| Setup failure and status | Existing private helper status decoder and no-target tests; C005 fixture checks invalid required mount | Legacy helper protocol is covered; no namespace status exists | Broken/forged status, no target marker, no FullHost retry | Legacy status tests pass; enhanced adapter absent |
| Cancellation/contention | Existing managed-process descendant tests; later namespace fixture uses two owned jobs and cleanup | Process-group timeout/cancel works | Namespace descendants and sockets are reaped, concurrent attempts are disjoint | Legacy lifecycle passes; enhanced lifecycle blocked |

All socket fixtures use loopback or unique abstract names, bounded reads,
finite deadlines, and owned threads/processes. No internet, user credentials,
real daemon socket, process flood, or host-wide network change is involved.

## Disposition

Only the legacy baseline fixture and its design oracle are complete here. Full
M004/M005 denial claims require a real accepted backend, exact host capability
capture, and observed negative/positive child effects. A test that merely
returns a connection error without a working fixture-side positive control is
not a denial result.
