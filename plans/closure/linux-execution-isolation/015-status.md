# Linux Execution Isolation C007 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/linux-execution-isolation/015-m004-m005-process-ipc-offline-fixture-and-denial-matrix.md`

Source subsystem roadmap:

- `plans/subsystems/linux-execution-isolation-roadmap.md#next-handoffs--explicit-test-only-vs-production-gates-2026-10-13`

Repository baseline reviewed: `73f2ee2` (after C005 test-only feasibility closure)

Implementation commits:

- `1616c6f32877b00d7348bdb3bfc387fbb807bfe6` — safe legacy Landlock process, loopback TCP, and abstract Unix socket baseline plus the C007 denial-fixture matrix.

## 1. Executive finding

The bounded C007 fixture-design and legacy-baseline slice is conditionally closed. A real child launched through CodeGG's existing Landlock helper successfully signaled a controlled same-UID host process, connected to a parent-owned loopback TCP listener, and connected to a parent-owned abstract Unix-domain socket. The child could not read `/proc/<host-pid>/environ` on this host because Yama `ptrace_scope=1` denied it; this is recorded as host policy, not Landlock process isolation.

The denial oracle and fixture design are recorded in `research/linux-execution-isolation/c007-baseline-and-denial-fixture-matrix.md`. No enhanced backend exists, so C007's full M004/M005 process, IPC, seccomp, and OfflineRequired denial matrix remains blocked. This closure makes no enhanced containment claim.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Show current Landlock process/PID limitation | `legacy_landlock_leaves_host_process_tcp_and_abstract_uds_reachable` | pass | Child sees PID via `kill(pid, 0)` and sends SIGTERM only to the owned sleeper fixture. |
| Show current loopback/network limitation | Same integration fixture | pass | One-byte request/ack succeeds against an ephemeral parent-owned loopback TCP listener. No external network was contacted. |
| Show current abstract IPC limitation | Same integration fixture | pass | One-byte request/ack succeeds over a unique parent-owned abstract AF_UNIX socket. |
| Distinguish host `/proc` policy from Landlock | Test observes `PermissionError`; `/proc/sys/kernel/yama/ptrace_scope` is `1` | pass | This denial is attributed to host Yama policy. |
| Report filesystem/network status truthfully | Helper frame in the integration test | pass | Backend is Landlock and the limits contain `no network isolation`. |
| Define deterministic future M004/M005 oracle | `research/linux-execution-isolation/c007-baseline-and-denial-fixture-matrix.md` | pass | Each planned denial includes a positive fixture control and bounded cleanup. |
| Prove enhanced process/IPC/network denial | No enhanced production backend | blocked | M004/M005 remain unimplemented; C007 full security disposition remains gated. |

Host: Ubuntu 24.04.5 LTS, x86_64, Linux `6.8.0-146-generic`; Landlock is enabled. AppArmor is enabled with current profile `unconfined`; SELinux is absent; host seccomp reports `0`; Yama `ptrace_scope=1`. The exact Landlock ABI was not emitted in this fixture and is not claimed here.

## 3. Production implementation evidence

No production implementation changed. The new code is a Linux-only integration fixture using the production sandbox spec/helper and a test-only research document. It creates only an owned `sleep` process, loopback listener, abstract socket, and temporary workspace. Cleanup reaps the child and listener threads have five-second deadlines.

## 4. Verification executed

### Commands run

```bash
CARGO_TARGET_DIR=/tmp/codegg-c005-test-target cargo test --test sandbox_containment --locked -- --test-threads=1
CARGO_TARGET_DIR=/tmp/codegg-c005-test-target cargo test --test sandbox_containment legacy_landlock_leaves_host_process_tcp_and_abstract_uds_reachable --locked -- --test-threads=1 --nocapture
CARGO_TARGET_DIR=/tmp/codegg-c005-test-target cargo test --test managed_process_descendants --locked -- --test-threads=1
python3 scripts/check_sandbox_contract.py
python3 scripts/check_execution_ownership.py
cargo fmt --all -- --check
scripts/verify.sh quick
```

### Results

All commands passed locally. The complete containment target passed 15/15, including the new baseline fixture; the managed-process descendant target passed 5/5. Quick verification passed, including workspace all-targets checking. Hosted CI was not run.

## 5. Invariant review

- The legacy helper remains filesystem-only; the test pins its explicit unrestricted-network limitation.
- Host process visibility and host-local sockets are not represented as denied under Landlock.
- No network policy, namespace backend, firewall rule, profile, approval, or authority changed.
- The fixture sends a signal only to its own process and uses only loopback and a generated abstract socket name.

## 6. Failure and recovery review

Fixture threads have bounded accept and read waits. The controlled host process is reaped; test cleanup kills it only if the child did not. Existing process-group cancellation/timeout regressions passed. No external route, flood, host daemon socket, or persistent host state is used.

## 7. Migration and compatibility review

No schema, protocol, or runtime behavior changed. The helper continues to report Landlock filesystem enforcement with unrestricted network. The future enhanced route must preserve the existing positive controls and distinguish host/Yama policy from sandbox enforcement.

## 8. Security review

Test inputs are synthetic and ephemeral. The child is untrusted for the duration of the fixture, but it can reach only test-owned sockets and the test-owned PID. The Yama denial is not promoted as a Landlock guarantee. Future negative network tests require a live positive control and must never rely on an absent route or DNS timeout as proof.

## 9. Documentation and operations

Added `research/linux-execution-isolation/c007-baseline-and-denial-fixture-matrix.md` with the baseline results and planned process, proc, FD, pathname/abstract socket, TCP, UDP, DNS, seccomp, lifecycle, and tool-compatibility oracle. The file explicitly marks all unimplemented enhanced checks as design-only.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | No qualified M004 PID/IPC/seccomp backend exists. | Host process, proc, inherited descriptor, and pathname socket denial remain unavailable. | Implement only under an accepted Linux ADR and prove effects in a native child. |
| medium | No qualified M005 offline network backend exists. | Loopback, UDP, IPv4/IPv6, DNS, and Unix-bridge denial remain unavailable. | Require M004 prerequisites, then run positive-controlled offline fixtures before any guarantee claim. |
| low | Landlock ABI number was not captured by this baseline fixture. | No new ABI claim is made. | Record it in a later native qualification record if needed. |

## 11. Roadmap disposition

<<<<<<< HEAD
C007's fixture-design and legacy-baseline slice is conditionally closed. The full C007/M004/M005 enforcement matrix remains blocked until real backend implementation and positive-controlled denial evidence exist. C008 and command M004 are independent; C006 remains blocked on ADR-0031 disposition and positive protected-mount evidence.
=======
C007's fixture-design and legacy-baseline slice is conditionally closed. The full C007/M004/M005 enforcement matrix remains blocked until real backend implementation and positive-controlled denial evidence exist. C008 and command M004 are independent; C006 remains blocked on ADR-0035 disposition and positive protected-mount evidence.
>>>>>>> d8da9933 (fix(plans): reconcile Linux ADR identity on current main)

## 12. Registry updates

- Move the baseline-fixture slice from dependency-ready work to recently closed as conditionally closed.
- Keep full M004/M005 production work blocked; this closure does not satisfy any enhanced enforcement gate.
- Unblock audit: no plan became ready. C008 and command M004 were already eligible independently.
