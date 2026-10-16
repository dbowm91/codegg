# M004 process and IPC implementation readiness

Status: implementation in progress; first exact-hosted positive denial matrix passed, broader compatibility/lifecycle gates remain open
Reviewed source baseline: current-main merge `188edb371759f40b291052f25598bdb8ab57702d` (2026-10-15); earlier hosted M004 implementation evidence remains pinned to its exact SHAs below.
Related plan: `plans/implementation/linux-execution-isolation/004-process-ipc-seccomp.md`
Related decision: accepted `plans/adrs/ADR-0035-composable-linux-execution-isolation.md`

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
| Seccomp compiler | `seccompiler` 0.5.0 is an upstream pure-Rust BPF compiler with an architecture-checked filter IR and `BpfProgram` output; Bubblewrap accepts raw cBPF program bytes through a seccomp FD. | Pinned as a Linux-only dependency and integrated with a fixed denylist covering namespace changes, legacy and new mount syscalls, tracing, and selected kernel control syscalls. Unit compilation and hosted denial of namespace and mount syscalls pass; rustup/Cargo/rustc compatibility remains open. Sources: [seccompiler API](https://docs.rs/seccompiler/0.5.0/seccompiler/), [Bubblewrap options](https://github.com/containers/bubblewrap/blob/main/bwrap.xml). |
| Exact-head hosted run `38760659016` | Ubuntu 24.04 hosted runner, kernel `6.17.0-1022-azure`, Bubblewrap `0.9.0`, x86_64, UID/GID 1001, AppArmor `unconfined`; `seccomp/actions_avail` includes `errno` and `allow`. The workflow changed ephemeral `kernel.apparmor_restrict_unprivileged_userns` from 1 to 0. | M004 native target passed 3/3: private proc/PID view, signal and abstract/pathname socket denial, ambient-FD closure, installed seccomp denial of namespace and mount syscalls, Git subprocess, and process-group descendant cancellation. The same run passed 11/11 protected-mount tests and 18/18 trusted-Git tests. This is candidate evidence on a runner whose user-namespace policy was relaxed; it does not admit a general host configuration. |
| Exact-head hosted run `38762601406`, source SHA `f2e4bd76` | Ubuntu 24.04.5 hosted runner, kernel `6.17.0-1022-azure`, Bubblewrap `0.9.0`, x86_64, UID/GID 1001, AppArmor `unconfined`; seccomp `errno` and `allow` available. The workflow changed ephemeral `kernel.apparmor_restrict_unprivileged_userns` from 1 to 0. | M004 target passed 3/3, including `rustup toolchain list` and `rustup run stable rustc --version` through the private proc view. Protected-mount passed 11/11 and trusted-Git passed 18/18. This qualifies the installed stable Rustup path on that adjusted runner only. |
| Exact-head hosted run `38764899922`, source SHA `c528ebff` | Ubuntu 24.04.5 hosted runner, kernel `6.17.0-1022-azure`, Bubblewrap `0.9.0`, x86_64, UID/GID 1001, AppArmor `unconfined`; seccomp `errno` and `allow` available. The workflow changed ephemeral `kernel.apparmor_restrict_unprivileged_userns` from 1 to 0. | M004 target passed 3/3, now including `/proc/<host-pid>/environ` and `/fd` invisibility, ptrace/process_vm denial, and cancellation of a descendant that starts a new session. Rustup/rustc smoke also passed. Protected-mount passed 11/11 and trusted-Git passed 18/18. Candidate evidence only on this adjusted runner. |
| New qualification work on the current-main integration | Added an M004 Cargo probe that executes real `cargo check --locked -p codegg-core` and `cargo test --locked -p codegg-core --lib` requests through `ManagedProcessService`; added a separate workflow job with `user.max_user_namespaces=0` that checks direct Bubblewrap refusal and a no-target-marker required launch. | Implementation is present; exact-head hosted results are pending. These results will qualify only the Cargo child path and disabled-userns fail-closed behavior. |

## Production child-owner audit

Two user-visible child owners currently bypass the M004 launch policy:

- `src/interactive_process.rs::spawn_unix_child` creates a PTY and invokes
  `tokio::process::Command` directly. It owns PTY stdio, controlling-terminal
  setup, resizing, and process-group cancellation; it does not use the private
  sandbox receipt or Bubblewrap M004 spec. The existing M003 PTY fixture
  constructs a Bubblewrap command separately and is not service-path evidence.
- `crates/egglsp/src/launch.rs::spawn_server_impl` invokes a resolved language
  server directly with piped stdio. The `egglsp` crate has no M004 request or
  receipt integration. Running `rust-analyzer --version` through a separate
  managed-process probe would establish binary compatibility only, not LSP
  protocol behavior or containment of CodeGG's production LSP child.

Therefore M004 cannot close by adding more standalone tool smokes. The PTY and
LSP owners must either be routed through a shared sandbox launch/receipt
boundary, or receive a separately authorized and reviewed equivalent boundary
that preserves their stdio contract. Until then they remain outside M004 and
must not be described as process-isolated.

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
a unit test for the action-list contract. Run `38759579735` then passed the
main denial probe but exposed invalid Python quoting in the separate
cancellation fixture. The quoting was corrected; exact-head run
`38760659016` passed all three M004 tests and the protected-mount/Git
regressions listed above.

## Remaining M004 production work

1. Add same-UID concurrent-session fixtures that attempt cross-session process
   and IPC access. Existing tests show host PID/proc entries are hidden and
   process/IPC namespaces are established, but do not yet prove cross-session
   isolation under concurrent service launches.
2. Run the new exact-head Cargo check/test probe and broaden it to a compiler
   subprocess tree if needed. Rustup and rustc smoke already pass only on the
   adjusted hosted candidate.
3. Implement service-level PTY routing through the contained launch owner,
   then verify interactive input/output, resize, signals, cancellation and
   cleanup. Do the same for the production `egglsp` child path and prove an LSP
   initialize/open/shutdown exchange inside the boundary.
4. Run the exact-head disabled-userns hosted job. It qualifies fail-closed
   behavior on that configured runner, not a generally supported Linux profile.
   The local AppArmor sysctl value was 1 but direct Bubblewrap userns creation
   succeeded, so that host is not evidence for denied-userns behavior.
5. Keep process isolation internal and unregistered until the service paths
   and M003 host-admission gates are resolved.

M005 can start after the M004 process/IPC and descriptor interface is stable.
It must separately carry operator-owned `OfflineRequired`, install a network
namespace before the target, and test IPv4/IPv6 TCP/UDP, DNS, loopback/host-local
listeners, pathname and abstract AF_UNIX, inherited sockets, and offline local
builds. Cgroup-v2 resource enforcement remains independent and requires actual
writable delegation. C002 local Git recovery follows proven M003–M005
containment; C003 remote Git follows C002 and M005/M006 mediation.
