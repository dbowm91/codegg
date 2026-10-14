# Linux Execution Isolation C007 — M004/M005 Process, IPC and Offline Network Denial Matrix

Status: **ready for test-fixture design**, production enforcement blocked on accepted Linux ADR, M003/C006 real mount backend, M004/M005 hard dependencies
Class: invariant / native qualification
Closure target: `plans/closure/linux-execution-isolation/015-status.md`.
Baseline: main @ `ec43e495c3619966ac33d0acccf7904812dd37bc` (2026-10-13 EDT). Long-term: `plans/000-long-term-specification.md` §§4/17/27/29, `plans/001-terminology-and-domain-model.md` §8, and `plans/003-planning-process.md`. Execution authority: ADR-0001/0004/0016; command classification: currently accepted `plans/adrs/ADR-0032-command-syntax-and-execution-authority.md` (canonical ID to be reconciled under C004). Existing Landlock ABI >=3 filesystem-only support must remain intact. This is a handoff plan, not a claim of implementation or test execution.

## 1. Objective

Prepare the security oracle **before** promoting any PID/IPC/seccomp/network namespace implementation. M004 must prove host processes, procfs, local sockets and inherited handles cannot be accessed; M005 must independently prove `OfflineRequired` denies *actual* host/external IPv4, IPv6, UDP and DNS reachability. Shell-regex/prefix matching, successful feature probes and a disconnected environment are not suitable test substitutes.

Parent handoffs: `004-process-ipc-seccomp.md`, `005-offline-network-isolation.md`, M002's typed `LinuxObtainedEnforcement`. Treat host test prerequisites as operational gates with exact capability diagnostics.

## 2. Native test corpus (no external internet required)

- Start controlled same-UID **fixture host process** and protected fixture sockets; sandboxed Rust/native, shell and Python probe children attempt `kill`, ptrace/process_vm, `/proc/<hostpid>/environ`, `/proc/<hostpid>/fd`, namespace fd links, host proc mounts and fd reuse. Verify legitimate own descendant processes, PID1 reaping, signal forwarding, timeout and cancellation continue functioning.
- Fixture IPC includes pathname and **abstract** AF_UNIX, host `/run`/DBus/SSH-agent-like sockets, Docker-like socket, PTY inheritance, ambient descriptors above fd2, inherited connected TCP socket, `SSH_AUTH_SOCK` env, and same-UID cross-session daemon/socket paths. Remember IPC namespace alone does not isolate abstract Unix sockets; compose network namespace or supported additional kernel controls before making the claim.
- Network host fixtures on loopback and a controlled route/network namespace for TCP IPv4/IPv6, UDP, DNS, IP literals, Unix socket bridges and namespaces. Check a *successful negative* (blocked connection) not merely `curl` exit failure due to no DNS/network target. Own in-sandbox loopback may function when explicitly scoped, but no host-loopback service should be reachable under OfflineRequired.
- Seccomp architecture and syscall filter installation test must show actual denied privilege-sensitive syscalls, while ordinary `cargo check`, `rustc`, Git status, Python, shell scripts and expected subprocesses still run. Handle Linux arch and no_new_privs, no unreviewed BPF for all kernels.
- Negative hosts: user namespace disabled, bubblewrap missing/untrusted, seccomp unavailable, `/proc` creation denied, LSM restricting namespace, broken status pipe and inherited fd hidden paths. Required setup failure must leave **no target marker** and must not retry as FullHost.

## 3. Work packages and partial dispositions

A. Design and register **test-only** native fixture protocol, safe ephemeral ports/paths, fake sockets and synthetic secret markers. Establish exact kernel/LSM/userns prerequisite table and bounded run time; do not run flood/fork bombs or real network exfiltration.

B. Add fixtures demonstrating baseline **legacy Landlock limits truthfully** (outside-root filesystem writes denied but network/IPC still unrestricted). Do not expect denial from legacy; mark it `Unrestricted` and prove later enhanced differences only on a qualified child.

C. As M004 production backend lands, run process/proc/IPC/seccomp matrix; an isolated PID mount and no host socket access both need actual negative probes. M005 may start only when M004 process/FD/sockets are qualified; then apply offline network namespace and prove IPv4/IPv6/UDP/DNS denial.

D. Cross-tool inherited authority checks: Bash, Python, compiled helper, agent-prepared script, model-editable Git hooks/filters and child process nesting all receive the same actual process/network ceiling; no change in route or argv0 widens it.

E. Report obtained dimensions independently in existing private status/audit; output/logs may record only structural metadata and sanitized synthetic fixture names. Capture cancellation, restart/orphan cleanup, two simultaneous jobs and no unexpectedly reused proxy/IPC socket.

## 4. Tests, evidence and acceptance

Linux native integration target(s) should be opt-in until safely qualified and capable host is available. Record exact kernel, Landlock ABI, namespace user/mount/pid/net/ipc creation, seccomp status, LSM, host fixtures and result. Also run `cargo fmt --all -- --check`, `cargo test -p codegg --test sandbox_containment -- --test-threads=1`, `cargo test -p codegg --test managed_process_descendants -- --test-threads=1`, `python3 scripts/check_sandbox_contract.py`, `python3 scripts/check_execution_ownership.py`, `scripts/verify.sh quick`.

The fixture-design slice may close when tests are deterministic and produce clear unsupported/negative host dispositions **without claiming enhanced containment**. M004/M005 and this full milestone's strict-positive enforcement disposition remain blocked until real child denies prohibited process/IPC/TCP/UDP/Unix socket effects and permits benign toolchain operations. Stop if tests rely on regex classification, inherited sockets escape, required failures launch a target, or fixture teardown could affect a non-fixture host process.
