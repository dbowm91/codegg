# M004 process and IPC implementation readiness

Status: implementation in progress; native positive qualification pending
Reviewed source baseline: branch head `1795b0d6` plus uncommitted M004 work
Related plan: `plans/implementation/linux-execution-isolation/004-process-ipc-seccomp.md`
Related decision: accepted `plans/adrs/ADR-0031-composable-linux-execution-isolation.md`

## Decision: keep M004 network policy independent

A PID namespace and an IPC namespace do not, by themselves, isolate abstract
AF_UNIX sockets. A network namespace would isolate those sockets, but using it
for M004 would also change the existing unrestricted-network behavior and
collapse M004 into M005. Landlock's ABI 6 scope attributes provide a narrower
mechanism: `LANDLOCK_SCOPE_ABSTRACT_UNIX_SOCKET` scopes abstract socket
connections to the same or nested Landlock domain, and `LANDLOCK_SCOPE_SIGNAL`
scopes outgoing signals. Kernel documentation dates ABI 6 to Linux 6.12 and
requires probing the Landlock ABI rather than inferring support from a distro
name or kernel string. Sources: [Landlock ABI 6 documentation](https://www.kernel.org/doc/html/v6.12/userspace-api/landlock.html),
[Landlock userspace API](https://docs.kernel.org/userspace-api/landlock.html).

The implementation composes the two Landlock scopes with Bubblewrap PID/IPC
namespaces and a procfs mounted inside the PID namespace. It leaves network
intent unchanged. A host without ABI 6 refuses before the target starts; it
does not weaken the request to PID/IPC-only containment.

## Evidence collected

| Host/evidence | Result | Meaning |
|---|---|---|
| Local Ubuntu x86_64, kernel `6.8.0-147-generic`, Bubblewrap `0.9.0` | Native managed-process test confirmed the required launch is refused before its marker target executes. The Landlock scope probe reports the ABI 6 scope mask as incompatible. | Correct negative behavior on a kernel predating ABI 6. No positive process/IPC claim for this host. |
| Linux qualification workflow | Added `.github/workflows/linux-m004-native.yml`; it records runner kernel/AppArmor/Bubblewrap facts and runs the managed-child M004 probe plus protected-mount/Git regressions. | Exact-head hosted positive evidence is pending. |
| Seccomp compiler | `seccompiler` 0.5.0 is an upstream pure-Rust BPF compiler with an architecture-checked filter IR and `BpfProgram` output; Bubblewrap accepts raw cBPF program bytes through a seccomp FD. | Pinned as a Linux-only dependency and integrated with a fixed denylist covering namespace changes, legacy and new mount syscalls, tracing, and selected kernel control syscalls. Unit compilation passes; native syscall/tool-compatibility probes are pending. Sources: [seccompiler API](https://docs.rs/seccompiler/0.5.0/seccompiler/), [Bubblewrap options](https://github.com/containers/bubblewrap/blob/main/bwrap.xml). |

The first hosted M004 run (`38754822245`, source SHA `7a507362`) reached the
positive Landlock ABI 6 branch, then failed because its signal probe expected
`EPERM`. In the private PID namespace the fixture PID is intentionally
unaddressable and Linux returned `ESRCH` (`ProcessLookupError`); no signal was
delivered. The oracle accepts either `EPERM` or `ESRCH` as denial while still
failing if the signal succeeds or the host marker is written.

The next run (`38757206944`, source SHA `3b62abd`) found a second issue: the
adapter probed `/proc/sys/kernel/seccomp/actions_avail` for the word `filter`,
but this kernel interface lists return actions (`allow`, `errno`, and similar),
not filter mode. The incorrect check rejected the otherwise capable host
before target execution, so no positive M004 result was obtained. The probe
now requires the `allow` and `errno` actions needed by this fixed policy, with
a unit test for the action-list contract. The hosted workflow records the
action list. Its native tests will also exercise a pathname socket under
`/tmp`, a Git subprocess smoke check, and descendant cancellation.

## Remaining M004 production work

1. Obtain a successful exact-head hosted native run for PID/proc invisibility,
   same-UID signal denial, abstract socket denial, private status, normal child
   execution, and managed cancellation.
2. Audit all inherited descriptors at the ManagedProcessService/helper boundary;
   retain only stdio, the bounded status/ack protocol, and explicit typed
   capabilities. The M004 parent now closes ambient descriptors before exec,
   preserving stdio and the reserved status/ack descriptors; its positive
   native probe is pending. Audit environment socket hints and test pathname
   sockets inside read/write bind roots before making any denial claim.
3. Prove the pinned seccomp policy through Bubblewrap with a denied namespace
   syscall and normal compiler/Python/Git behavior; review the syscall list and
   add only compatibility-driven exceptions with explicit evidence.
4. Exercise descendants, timeout/cancellation, concurrent sessions, and PTY
   status/resize behavior. Verify a setup failure produces no target marker.
5. Only after those results, close M004 or record narrowly scoped external host
   limitations. Until then the process mode remains an internal opt-in with no
   production admission.

M005 can start after the M004 process/IPC and descriptor interface is stable.
It must separately carry operator-owned `OfflineRequired`, install a network
namespace before the target, and test IPv4/IPv6 TCP/UDP, DNS, loopback/host-local
listeners, pathname and abstract AF_UNIX, inherited sockets, and offline local
builds. Cgroup-v2 resource enforcement remains independent and requires actual
writable delegation. C002 local Git recovery follows proven M003–M005
containment; C003 remote Git follows C002 and M005/M006 mediation.
