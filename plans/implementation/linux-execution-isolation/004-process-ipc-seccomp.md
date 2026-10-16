# Linux Execution Isolation M004 — Process, IPC, Procfs and Syscall Isolation

Status: active (internal PID/IPC/proc/FD/seccomp implementation has exact-hosted positive probes, including concurrent namespace separation; broader compatibility, PTY routing, and host-admission gates remain open)
Primary class: capability
Repository baseline: current-main merge `188edb371759f40b291052f25598bdb8ab57702d` (2026-10-15); implementation work began at `1795b0d6`. Source roadmap: `plans/subsystems/linux-execution-isolation-roadmap.md`. Long-term requirements: `plans/000-long-term-specification.md` §§4,17,27,29 and `plans/001-terminology-and-domain-model.md` §8. Applicable accepted ADRs: ADR-0001, ADR-0002, ADR-0004, ADR-0016, ADR-0035. Accepted ADR-0034 governs representation-independent command authorization. Real Linux backend promotion still requires native qualification and rollout gates. Target closure: `plans/closure/linux-execution-isolation/004-status.md`. PID/IPC/proc/FD and architecture-checked seccomp enforcement are implemented behind an internal opt-in and have exact-head hosted denial and ordinary-tool probes; broader host qualification, production PTY/LSP routing, and full toolchain compatibility remain open.

## 1. Objective

Provide an opt-in native Linux child execution tier with **real process and IPC isolation**, an intentionally constructed procfs, controlled inherited descriptors and constrained kernel syscall surface. Preserve Codegg's parent process supervision and helper private status channel; prohibit reachability to the user daemon and other sessions through same-UID host processes or sockets unless explicitly granted by authenticated policy.

## 2. Current evidence

Landlock constrains filesystem grants and inherits across exec. ABI 6 adds signal and abstract Unix socket scopes; the local Ubuntu 6.8 kernel lacks them and the required request refuses before target execution. Bubblewrap composes those scopes with PID/IPC namespaces, a private proc view, and an architecture-checked `seccompiler` denylist behind an internal opt-in. The M004 parent closes inherited FDs above stdio and reserved status/ack descriptors. Exact-head hosted run `38764899922` on `c528ebff` passed the M004 native target (3/3), including host `/proc/<pid>/environ`/`fd` invisibility, ptrace/process_vm denial, detached-session descendant cancellation, and Rustup/rustc smoke. Follow-up run `38767363902` on `9f692258` passed M004 (4/4), protected-mount (11/11), and trusted-Git (18/18) tests, adding proof that concurrent launches receive distinct PID and IPC namespace inodes. Both runs used Ubuntu 24.04.5, kernel `6.17.0-1022-azure`, Bubblewrap `0.9.0`; the workflow relaxed that ephemeral runner's unprivileged-userns AppArmor sysctl. Remaining gaps include broader Cargo/compiler compatibility, PTY service routing, and production admission. The interactive PTY owner does not yet consume this M004 policy; the protected-mount PTY test only exercises a manually constructed Bubblewrap first hop. The daemon and tool services may expose local sockets; process-group cancellation alone is not an isolation boundary.

Exact reconciled-head qualification on `5f1f998616f8727e4a84aea37ff6125884da4d19`
passed hosted run [38794947434](https://github.com/dbowm91/codegg/actions/runs/38794947434)
on Ubuntu 24.04.5. The M004 native suite passed 6/6, including concurrent
distinct PID/IPC namespaces, host-proc and host-signal/abstract-socket denial,
descendant cancellation, and a contained workspace check plus managed-process
tests. The separate restricted-userns job proved refusal before target start.
The same run passed protected-mount regressions 11/11 and trusted-Git policy
regressions 18/18. This qualifies the current internal process/IPC test tier;
it does not qualify production PTY/LSP routing or change the no-admission
decision for the protected-mount backend.

### Exact-head post-reconciliation qualification (2026-10-16)

After reconciling the branch with current `main`, hosted M004 run
[38869606745](https://github.com/dbowm91/codegg/actions/runs/38869606745)
on `7312acad74f4f6f10ee6b505a61fc9ce51f3f7ac` passed the native process/IPC
target 9/9. This includes host-process/proc/IPC denial, cancellation and
concurrency probes, offline-network denials with host positive controls, a
contained offline `cargo metadata` plus full-workspace `cargo check`, and a
contained workspace check plus managed-process regressions. The protected-
mount and trusted-Git regression targets passed 11/11 and 18/18. The separate
restricted-userns run passed the required refusal-before-target probe.

Exact-head full CI
[38869639397](https://github.com/dbowm91/codegg/actions/runs/38869639397)
passed 14,020 tests with 7 skipped; all ADR identity, sandbox, execution
ownership and other workflow guards, formatting, Clippy, Eggwork fixture
prebuild, and trusted helper build passed. The offline workspace test's initial
attempt used invalid `cargo metadata --workspace` syntax and failed; commit
`7312acad` corrected the invocation. The final hosted result is for the
corrected code.

This qualifies the current internal process/IPC and offline-network test
candidate on the recorded hosted runner. It does not close M004: constrained
PTY creation still refuses before spawn, and constrained LSP turns remain
unbound from the shared FullHost service because there is no supervised duplex
LSP process owner carrying the M004 launch scope. Supported-host qualification
and actual production-owner routing remain hard closure gates. M005 remains an
internal candidate, and C010's M003 no-admission decision is unchanged.

### Exact-head DNS and object-view regression qualification (2026-10-16)

After adding the host-configured resolver and changing-answer DNS fixtures,
hosted Linux run
[`38928962604`](https://github.com/dbowm91/codegg/actions/runs/38928962604)
on `863537c0f36e0f71c4b1f784137ed69ff8ad4295` passed the native M004/M005
target 9/9, protected-mount regressions 11/11, trusted-Git regressions 20/20,
and the separate restricted-userns refusal-before-target probe. The 9/9
includes host DNS positive control followed by denial to the configured
resolver and a loopback resolver that returns different A records for the
same query, again denied from the contained child. It also retains the offline
Cargo metadata/workspace-check candidate. macOS target compile
[`38928962564`](https://github.com/dbowm91/codegg/actions/runs/38928962564)
passed. Exact-head full CI run `38929001163` failed in the existing typed Git
log service test; its focused test passed locally, but that hosted failure is
not classified as transient. Same-SHA retry `38931841531` exposed a separate
stale background-shell integration fixture: its `BashTool` had no registered
workspace root/allowed-path binding and submitted relative `.`. The fixture
now uses the canonical registered workspace, and its complete integration
file passed locally 12/12. That isolated correction is commit `7646ed68`;
fresh full CI `38934845574` on that head is pending. Full-suite
reconciliation remains open until the latest run completes.

This additional native evidence closes the DNS fixture gap only. The PTY
service still refuses constrained profiles before `openpty`/spawn, and the
LSP service still has no distinct daemon-owned constrained runtime. No M004
production owner or M005 production network-enforcement claim is promoted.

### Post-merge exact-head owner-boundary qualification (2026-10-16)

After reconciling the feature branch with current `main`, hosted Linux run
[`38935343767`](https://github.com/dbowm91/codegg/actions/runs/38935343767)
on merge head `c01b2dfc` passed the process/IPC/offline target 9/9, protected
mount regressions 11/11, and trusted-Git regressions 20/20. Its separate
restricted-userns job passed refusal-before-target. The DNS endpoint,
changing-answer resolver, offline Cargo workspace, process/IPC, and object-view
cache regressions are included. Hosted macOS target compile
[`38935350738`](https://github.com/dbowm91/codegg/actions/runs/38935350738)
also passed. These are native mechanism and cross-platform compile results;
they do not qualify production PTY/LSP owners or admit a host configuration.

Exact-head full CI on follow-up test-fixture head `91a5407e` is
[`38937869443`](https://github.com/dbowm91/codegg/actions/runs/38937869443).
The ADR identity/link, sandbox contract, execution ownership, other workflow
guards, formatting, Clippy, Eggwork fixture preparation, and trusted helper
build passed. Workspace tests failed two command-routing ownership cases
because their `BashTool` fixtures submitted relative `.` as the workspace
root. Both cases passed after the test helper registered and bound its
canonical repository root; the focused integration file passed 21/21 locally.
A fresh full CI run for that correction is required before reconciliation is
complete. The fresh run `38941049575` passed the corrected owner-test fixtures
but failed `git_service::tests::execute_status_returns_structured_payload`:
the structured branch lookup returned `unknown` while raw status succeeded.
The ref-only `branch --show-current` request was unnecessarily subjected to
object-store admission. It is now classified as not reading object contents,
and object-store validation is deferred until a command actually needs an
object view. The new alternate regression and the complete egggit suite pass
locally (108 passed, 1 ignored), as does the focused structured-status test;
fresh full CI remains required. The earlier hosted Git-log failure did not
recur in the 6,704 tests run before fail-fast in `38937869443`; it remains
classified as an earlier unreproduced failure, not a confirmed transient.

The PTY service still refuses constrained profiles before `openpty`/spawn,
and constrained LSP turns remain unbound from the shared FullHost service
because no supervised duplex LSP process owner carries the M004 launch scope.
No production PTY/LSP owner or M005 production network-enforcement claim is
promoted. C010's no-admission decision remains unchanged.

## 3. Invariants

User code is never launched before the requested PID/IPC/mount/proc/seccomp features are installed. No host procfs bind or leaking daemon socket from `/run`, `/tmp`, home or inherited environment is acceptable under process-isolated mode. Only explicitly documented device/pty I/O is exposed. Parent-facing stdin/stdout/stderr and the private status channel are the only narrowly inherited descriptors unless a typed operator-approved capability demands more. Signal behavior and child PID1 reaping do not break `ManagedProcessService` cancellation, timeout or worker accounting. No root privileges or daemon self-restriction.

## 4. Production changes

- Build PID and IPC namespace with a private proc mount *after* successful namespace setup. Mount an empty/limited `/sys` and `/run` view rather than reusing host trees; account for glibc, dynamic linker, DNS files, device nodes and devpts/TTY compatibility. Prevent `/proc/<host-pid>/fd`, `/proc/sysrq-trigger`, namespace escape via proc magic links, and signal/ptrace cross-session paths.
- Audit both filesystem pathname Unix sockets and **abstract AF_UNIX sockets**. A PID/IPC namespace alone does not guarantee abstract-socket isolation. This implementation uses the Landlock ABI 6 signal and abstract-socket scopes, in addition to private PID/IPC namespaces and mount exposure; hosts lacking ABI 6 refuse before target execution. This preserves the network policy boundary for M005 rather than implicitly making M004 offline. No blanket host `/run` mount.
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
CODEGG_RUN_M004_BWRAP_NATIVE=1 cargo test --locked --test linux_m004_process_ipc process_tier_hides_host_proc_and_blocks_host_signal_and_abstract_socket -- --nocapture
CODEGG_RUN_M004_CARGO_NATIVE=1 cargo test --locked --test linux_m004_process_ipc process_tier_runs_workspace_check_and_managed_process_tests -- --nocapture
CODEGG_RUN_M004_RESTRICTED_USERNS=1 cargo test --locked --test linux_m004_process_ipc required_process_tier_fails_closed_when_user_namespaces_are_restricted -- --nocapture
python3 scripts/check_sandbox_contract.py
scripts/verify.sh quick
```
The positive host filters require Landlock ABI 6 and usable namespaces. The
restricted-userns filter runs on a separate ephemeral runner with
`user.max_user_namespaces=0`, verifies a direct harmless Bubblewrap probe is
denied, then proves that an M004 request leaves no target marker. The Cargo
filter runs `cargo check --locked --workspace` and the root crate's managed-
process unit tests as M004 children. The Cargo probe preserves rustup's
toolchain dispatch explicitly after executable canonicalization and sets the
installed Rust/Cargo roots, target linker, and C build-script compiler inside
the contained command because the helper clears ambient environment. This
exercises workspace compilation, build-script/proc-macro execution, writable
target artifacts, and tests of the service that owns the sandbox launch.
The nested managed-process filter skips only tests named `protected_mount_`:
those fixtures test host Bubblewrap discovery and recursive protected-mount
launch, which is unavailable from inside the M004 child by design. The same
hosted workflow runs those regressions separately after the M004 probe passes.
Hosted exact-head results are required; compilation or a skipped opt-in test is
not qualification.

The `egglsp` launcher and `InteractiveProcessService` currently spawn children
outside `ManagedProcessService` and do not consume the M004 policy. Their
production integration is a hard M004 closure gate; a PTY first-hop fixture or
standalone LSP smoke does not satisfy it. Routing follows the daemon-resolved
execution policy: unconstrained (`FullHost`) launches preserve the current
path, while a constrained profile must carry trusted policy from the daemon to
the launch owner and either use the qualified M004 boundary or refuse before
target execution. The request payload must not gain a client-controlled
"constrained" switch. Qualification must exercise both branches so an
unconstrained launch remains compatible and a requested constrained launch
cannot escape through a specialized protocol owner.

PTY policy association is daemon-owned and principal-scoped. At create time,
the daemon resolves the transport-bound principal's `RuntimePreference` and
passes the resulting `SandboxProfile` to `InteractiveProcessProtocol`; no
client DTO field selects or widens the profile. The protocol refuses
`ReadOnly` and `WorkspaceWrite` requests before scheduler admission or target
spawn until PTY creation is owned by the qualified M004 launcher. `FullHost`
retains the existing PTY path. A preference-store failure refuses creation.
This association does not claim constrained PTY support and does not attach a
client-selected profile to a process.

Agent turns carry a daemon-resolved sandbox profile. `LspTool` and the shared
daemon `LspService` are available only to FullHost turns; constrained turns
receive disabled LSP tools and no shared service or preview adapter. The
ownership decision is that a future contained LSP runtime must be a distinct
daemon-owned service instance whose immutable launch scope includes profile,
workspace, server identity, and generation. That service must own launch,
restart, cancellation, and shutdown for every LSP child; it must not look up
or restart a FullHost process. Its launch adapter must preserve supervised
JSON-RPC stdin/stdout and stderr while applying the qualified M004 boundary.
The current LSP launcher does not support this duplex contained-process
contract, so constrained LSP process creation remains refused before spawn.
End-to-end tests must pin FullHost start/restart compatibility and prove a
constrained turn cannot create, reuse, or restart a FullHost LSP process.

The current corrective refuses constrained LSP at session-tool construction:
it registers unavailable `lsp`/`lsp_read` tools, does not bind the shared LSP
service or preview adapter, and omits LSP prompt-context collection. FullHost
turns keep the existing service. This closes the cross-profile reuse path by
refusing before a specialized launch, but it is a fail-closed compatibility
state, not positive M004 LSP qualification. Production PTY now resolves the
principal's daemon-owned profile and refuses constrained creates before spawn;
FullHost behavior remains compatible. M004 stays active until a contained
duplex LSP owner and contained PTY route are qualified on supported and
restricted-userns host configurations.

`egglsp::LspService` now accepts one `LspProcessLauncher` when constructed and
routes both initial launches and restart generations through that same
service-owned launcher. Existing services use the direct FullHost adapter.
This provides the injection boundary for a future daemon-owned constrained
LSP instance while preserving duplex pipe ownership. No constrained adapter is
currently installed, and the existing constrained refusal remains in force;
the interface change is not execution containment evidence.

The next exact-code-head hosted qualification was dispatched on
`0ddafd40609be6e57d09046584e3478d79c7ca63`. Linux protected-mount run
[`38944344509`](https://github.com/dbowm91/codegg/actions/runs/38944344509)
passed 11/11 protected-mount tests and 20/20 trusted-Git tests; the separate
macOS target compile [`38944358617`](https://github.com/dbowm91/codegg/actions/runs/38944358617)
passed. The restricted-userns refusal job in Linux M004 workflow
[`38944358636`](https://github.com/dbowm91/codegg/actions/runs/38944358636)
passed. The positive process/IPC job in `38944358636` remains in progress. Full CI
[`38944358683`](https://github.com/dbowm91/codegg/actions/runs/38944358683)
failed at `git_service::tests::changed_files_returns_payload` (expected a
structured `ChangedFiles` payload, received `None`) after 1,497 tests had
passed. The focused test and all 25 Git-service tests pass locally; 100
repeated focused executions also pass. A same-SHA full-CI rerun is underway.
This hosted failure remains unresolved pending the rerun; no result is inferred
from partial logs.

## 9. Acceptance criteria

An opted-in child cannot enumerate or inspect host proc, signal/ptrace a same-UID fixture process, reach a withheld Codegg-daemon/session socket or abstract UDS, or install unsafe namespaces. It **can** run expected child shell/compiler/test operations and its descendant processes remain cancellable/reaped. Cargo check/test workflows pass through the M004 child boundary. A host with user namespaces disabled demonstrably refuses before target execution. Production PTY and LSP process creation use the canonical M004 launch/receipt owner before closure; direct-spawn service paths are not treated as contained. Every advertised seccomp/process/IPC guarantee is supported by a real negative and positive probe. Required failure leaves no target execution; legacy behavior unaffected.

## 10. Stop conditions

Stop if the selected namespace composition cannot protect abstract UDS while preserving status/PTY contract, seccomp filter would block essential harmless tools without a bounded workaround, or cleanup cannot reliably own namespace descendant life. Do not claim network isolation from PID/IPC alone; M005 owns explicit network controls.
