# Linux Execution Isolation M004 — Closure Status

Status: blocked

Source implementation plan:

- `plans/implementation/linux-execution-isolation/004-process-ipc-seccomp.md`

Source subsystem roadmap:

- `plans/subsystems/linux-execution-isolation-roadmap.md#6-dependency-graph`

Repository baseline reviewed: `2da93a18`

Implementation commits or pull requests:

- `2da93a18` — added an opt-in Landlock ABI 6+ signal and abstract Unix socket
  scope to the existing helper path, with separate requested/obtained IPC
  status and fail-closed behavior.
- Hosted native Linux product test: GitHub Actions run `38507455534`.
- Hosted macOS library compile: GitHub Actions run `38507455583`.

## 1. Executive finding

M004 is not complete. The implemented slice uses Landlock ABI 6+ to prevent a
child in a separate Landlock domain from signaling host processes or connecting
to host abstract Unix sockets. It is opt-in, fails before target execution when
the required ABI is unavailable, and is reported as IPC scope separately from
PID/process isolation. M004 still lacks the required PID namespace, private
procfs, seccomp policy, general inherited-descriptor sanitation, pathname Unix
socket boundary, and full descendant lifecycle evidence.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Host abstract AF_UNIX socket is unreachable under opt-in scope | Hosted real-child helper test `38507455534`, Ubuntu 24.04.5, kernel `6.17.0-1022-azure`, Landlock ABI 7; host listener accepted no connection | pass for this channel | Product test runs the actual CodeGG helper and asserts the child denial and obtained guarantee. |
| Same-UID host process signal is denied | Same hosted helper test; fixture did not receive SIGUSR1 and target recorded EPERM/EACCES | pass for this channel | Does not prove PID isolation, ptrace denial, or all signal behaviors. |
| Required scope on unsupported ABI refuses before target | Local helper integration test on kernel `6.8.0-146-generic`, Landlock ABI 4 | pass | Marker remained absent on setup failure. |
| IPC scope is reported independently from PID/process | `LinuxLaunchRequest`/`LinuxObtainedEnforcement` unit test | pass | `ipc=Applied`; `process=NotRequested`. No broad process claim is emitted. |
| PID namespace, private procfs and host PID invisibility | No namespace backend or private proc mount | not met | Host proc and PID visibility remain outside this slice. |
| Conservative seccomp with compatibility and negative syscall probes | No seccomp installer/profile | not met | Host feature discovery remains diagnostic only. |
| Inherited FD and pathname-socket sanitation | No generalized descriptor filtering or pathname-socket boundary | not met | No protection claim is made. |
| Process-tree namespace reaping/cancellation and ordinary-tool qualification | Existing managed-process behavior only | not met | No new namespaced process owner was introduced. |
| macOS compatibility of the default path | Hosted run `38507455583` compiled the CodeGG library for `x86_64-apple-darwin` | pass | Runtime behavior unchanged; non-Linux explicit scope requests fail closed. |

## 3. Production implementation evidence

`SandboxConfig` and its helper-consumed `SandboxLaunchSpec` carry an explicit
default-off Linux IPC scope request. Linux converts it to a required typed IPC
dimension, keeps process namespace state `NotRequested`, and applies Landlock
`Scope::AbstractUnixSocket | Scope::Signal` in the existing trusted helper.
Helper status reports the guarantee only after enforcement. A required request
without the private status guarantee is rejected. Non-Linux managed-process
requests and Seatbelt application reject the Linux-only option before target
execution. The default Landlock profile and network behavior are unchanged.

The option has no new CLI, persisted profile or end-user selection. It is a
narrow infrastructure slice and does not provide the M004 execution tier.

## 4. Verification executed

### Commands run

```bash
cargo test --locked -p codegg --test sandbox_landlock scoped_ipc_denies_host_abstract_sockets_and_signals_or_refuses_before_target -- --test-threads=1 --nocapture
cargo test --locked -p codegg --lib security::sandbox::linux_launch::tests::ipc_scope_is_required_and_only_reported_from_private_helper_guarantee
cargo test --locked -p codegg --lib security::sandbox::tests::launch_spec_maps_workspace_write_to_write_roots
cargo check --locked -p codegg --lib --target x86_64-pc-windows-gnu
python3 scripts/check_sandbox_contract.py
python3 scripts/check_sandbox_policy_wiring.py
python3 scripts/check_adr_integrity.py
cargo fmt --all -- --check
```

### Results

- Local ABI 4 helper integration: 1 passed through the required-scope
  fail-before-target branch.
- Launch-contract IPC-obtained unit test: 1 passed.
- Sandbox configuration launch-spec unit test: 1 passed.
- Hosted run `38507455534`: the capability probe found Landlock ABI 7 and its
  real-child Rust integration test passed (1 passed, 0 failed). The test
  observed host abstract-socket denial and cross-domain signal denial.
- Hosted macOS compile run `38507455583`: passed.
- Windows GNU library cross-check: passed with existing unrelated warnings.
  A broad `cargo check --tests --target x86_64-pc-windows-gnu` was also tried
  but is not a valid repository-wide target because Unix-only tests and the
  Unix `main.rs` test module are compiled by that invocation; it failed on
  Unix-only APIs in `sandbox_containment` and `src/main.rs`, not on this change.
- Sandbox contract, sandbox policy wiring, ADR integrity and rustfmt checks:
  passed.

## 5. Invariant review

- Requested IPC scope is separate from PID/process scope and its obtained fact.
- Older or unsupported Landlock kernels refuse the required option before
  target execution.
- No namespace, proc, seccomp, descriptor, pathname-socket, or network claim is
  inferred from IPC scope success.
- Default behavior and existing unrestricted network semantics remain
  unchanged.

## 6. Failure and recovery review

The option is enforced in the existing one-shot helper before target exec;
the existing managed-process owner retains target timeout, cancellation and
output ownership. No extra supervisor, namespace PID1 or cgroup is created.
The hosted product test uses a same-UID abstract-socket/signal fixture. Full
namespace descendant reaping, concurrent IPC isolation and cancellation
evidence remain outstanding with the unimplemented M004 tier.

## 7. Migration and compatibility review

The new launch-spec boolean is serde-default false. No database, public DTO,
approval mode, profile default or CLI migration was added. Linux-only requests
fail on non-Linux platforms; existing false/default requests compiled for
Windows GNU and macOS.

## 8. Security review

The guarantee is limited to abstract AF_UNIX sockets and signals subject to
Landlock ABI 6+ scope semantics. Pathname sockets, ptrace, host procfs,
inherited descriptors, network egress and syscalls remain outside the
guarantee. The private helper status channel remains the authority for the
obtained fact.

## 9. Documentation and operations

- Updated `architecture/security.md`, `docs/security-semantics.md`, and the
  permission skill with the exact opt-in scope and limitations.
- Added a real-child Linux helper test and temporary hosted-runner workflow.
- The hosted workflow is temporary evidence collection and must be removed
  after its completed run is recorded.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | PID/process namespaces, private procfs, seccomp, general FD sanitation and pathname-socket boundary are absent | M004's required process and IPC containment tier is unavailable | Implement the M004 ordered work packages and prove each boundary on native Linux. |
| high | No full namespace child lifecycle or tool compatibility evidence | Cancellation/reaping and ordinary tool execution are unqualified for a namespace tier | Add process-tree, cancellation, concurrency and toolchain acceptance tests. |
| medium | IPC scope has no user/operator selection surface | The implementation slice is not yet an operator-visible capability | Add only through the separately reviewed M008 admission path after full M004 closure. |

## 11. Roadmap disposition

M004 remains blocked. The Landlock IPC scope is a useful independently verified
subdimension, but it cannot unblock M005 offline networking or claim the
process-isolated tier. M003 remains independently blocked on its protected
mount-view design.

## 12. Registry updates

- Record this partial M004 implementation and keep the milestone blocked.
- Keep M005 blocked on full M004 native process/network prerequisite evidence.
- Keep C002, C003, M001 strict closure and M008 blocked on their existing
  dependency graph.
