# Linux Execution Isolation M004 — Process, IPC, Procfs and Syscall Isolation

Status: blocked (hard: M002; interface: M003 protected view)
Primary class: capability
Repository baseline: `main` at `8535f6d7c2e68f5d134ba0c73bd146ca8bc0bd03` (2026-10-13). Source roadmap: `plans/subsystems/linux-execution-isolation-roadmap.md`. Long-term requirements: `plans/000-long-term-specification.md` §§4,17,27,29 and `plans/001-terminology-and-domain-model.md` §8. Applicable accepted ADRs: ADR-0001, ADR-0002, ADR-0004, ADR-0016. Proposed design gate: ADR-0022; do not treat it as accepted. Target closure: `plans/closure/linux-execution-isolation/NNN-status.md` (use milestone number). No code or tests in this plan are represented as already implemented.

## 1. Objective

Provide an opt-in native Linux child execution tier with **real process and IPC isolation**, an intentionally constructed procfs, controlled inherited descriptors and constrained kernel syscall surface. Preserve Codegg's parent process supervision and helper private status channel; prohibit reachability to the user daemon and other sessions through same-UID host processes or sockets unless explicitly granted by authenticated policy.

## 2. Current evidence

Landlock constrains filesystem grants, inherits across exec and asserts no_new_privs, but does not install PID/IPC namespaces or seccomp filters, stop arbitrary signals to accessible same-UID host processes, or close already open descriptors. `policy::tool_read_allowances` names broad `/proc`, `/sys`, `/var/run`. Existing tests explicitly record inherited-FD limitations. The daemon and tool services may expose local sockets; process-group cancellation alone is not an isolation boundary.

## 3. Invariants

User code is never launched before the requested PID/IPC/mount/proc/seccomp features are installed. No host procfs bind or leaking daemon socket from `/run`, `/tmp`, home or inherited environment is acceptable under process-isolated mode. Only explicitly documented device/pty I/O is exposed. Parent-facing stdin/stdout/stderr and the private status channel are the only narrowly inherited descriptors unless a typed operator-approved capability demands more. Signal behavior and child PID1 reaping do not break `ManagedProcessService` cancellation, timeout or worker accounting. No root privileges or daemon self-restriction.

## 4. Production changes

- Build PID and IPC namespace with a private proc mount *after* successful namespace setup. Mount an empty/limited `/sys` and `/run` view rather than reusing host trees; account for glibc, dynamic linker, DNS files, device nodes and devpts/TTY compatibility. Prevent `/proc/<host-pid>/fd`, `/proc/sysrq-trigger`, namespace escape via proc magic links, and signal/ptrace cross-session paths.
- Audit both filesystem pathname Unix sockets and **abstract AF_UNIX sockets**. A PID/IPC namespace alone does not guarantee Unix socket isolation; use appropriate mount exposure, network namespace or supported Landlock ABI 6 socket/signal restrictions as additional controls, with an explicit test when the host ABI is too old. An offline network namespace may need to be composed for abstract-socket isolation before claiming this tier. No blanket host `/run` mount.
- Install a conservative seccomp filter, architecture-checked, under no_new_privs before target exec. Constrain kernel-risk interfaces (namespace/mount creation, ptrace/process_vm, keyrings, bpf/perf, privileged syscalls and unwanted socket families as compatible with supported toolchains). Choose deny action/errno deliberately; never silently accept an uninstalled filter. Retain ability to run ordinary rustc/git/cargo/python and fork child tests.
- Sanitize file-descriptor inheritance and environment for sandboxed children: audit `SSH_AUTH_SOCK`, `DBUS_SESSION_BUS_ADDRESS`, `WAYLAND_DISPLAY`, `DOCKER_HOST`, systemd/session sockets, agent forwarding, inherited fd 3+, socket activation and PTY. Treat absolute paths in those variables as potential mounts/authority, not trusted inputs. Do not close the private setup status/spec plumbing before the helper has completed its protocol.
- Ensure process/session ownership with namespace PID1 or suitable minimal init, correct signal forwarding, wait/reap, output bounds and cancellation under `ManagedProcessService`. Extend typed obtained status with process/IPC/seccomp separately, not a blanket `sandboxed` boolean.

## 5. Ordered work packages

A. Discover all daemon and test socket/FD locations; construct hostile target fixtures probing host process signal/ptrace, `/proc`, abstract and pathname UDS, inherited descriptors and service endpoints.

B. Implement private mount/PID/IPC/proc initialization and fail-closed setup with M002 adapter; prove no target marker on unsupported features. Include an explicit test of whether network namespace is a hard dependency for claimed abstract socket isolation.

C. Implement seccomp policy with maintained filter installation path and kernel/version/arch checks; test refusal when requested filter cannot be installed. Avoid hand-written broad BPF with no disassembly/review/test harness.

D. Verify ordinary tools, interactive PTY applicability, subprocess descendant handling, full process-tree cleanup and no shared-session interference.

E. Update docs, static guard, capability report and closure. Record each unisolated channel honestly if staged feature remains incomplete.

## 6. Failure, cancellation, restart, contention

Setup/unknown filter or missing namespace is terminal before target. Multi-process helpers must forward signals, reap children and release namespace mounts; cancellation cannot leave a descendant running in an orphaned namespace and cannot signal unrelated daemon/host process groups. Restart cleanup must not kill another session with reused PIDs; use owned handles/identity. Two jobs must not share IPC namespace/abstract UDS reachability inadvertently.

## 7. Compatibility/migration

Opt-in only. No change to basic Landlock profile or host daemon. Preserve current environment/command behavior in legacy mode. An external interactive debugger, terminal socket, Docker engine or language server may require a specifically authorized exception or separate trusted execution tier; these are not implicit permissions.

## 8. Tests and verification

Real-child probes: host PID enumeration and `/proc/<pid>/environ`, FD traversal, ptrace/process_vm and signals against a non-sensitive same-uid fixture process; UDS across abstract/path namespaces, session bus/SSH socket/Docker test socket; normal own-child signals and subprocesses; seccomp-filter installation and blocked syscall; namespace escape via unshare/mount; concurrent sessions and nested child/signal cancellation. Record kernel config, LSM, namespace feature availability and seccomp status. Negative no-userns/no-seccomp fixtures must refuse. Maintain tooling smoke: shell, Git, Cargo, Python, rustc and a bounded test runner.

```bash
cargo fmt --all -- --check
cargo test -p codegg --lib managed_process
cargo test -p codegg --test managed_process_descendants -- --test-threads=1
cargo test -p codegg --test sandbox_containment -- --test-threads=1
python3 scripts/check_sandbox_contract.py
scripts/verify.sh quick
```
Add focused Linux-only process/IPC/seccomp tests and record the exact command in closure. Do not mark only unit tests as mechanism evidence.

## 9. Acceptance criteria

An opted-in child cannot enumerate or inspect host proc, signal/ptrace a same-UID fixture process, reach a withheld Codegg-daemon/session socket or abstract UDS, or install unsafe namespaces. It **can** run expected child shell/compiler/test operations and its descendant processes remain cancellable/reaped. Every advertised seccomp/process/IPC guarantee is supported by a real negative and positive probe. Required failure leaves no target execution; legacy behavior unaffected.

## 10. Stop conditions

Stop if the selected namespace composition cannot protect abstract UDS while preserving status/PTY contract, seccomp filter would block essential harmless tools without a bounded workaround, or cleanup cannot reliably own namespace descendant life. Do not claim network isolation from PID/IPC alone; M005 owns explicit network controls.
