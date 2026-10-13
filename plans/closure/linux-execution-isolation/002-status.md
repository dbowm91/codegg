# Linux Execution Isolation M002 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/linux-execution-isolation/002-composable-linux-launch-contract.md`

Source subsystem roadmap:

- `plans/subsystems/linux-execution-isolation-roadmap.md`

Repository baseline reviewed: `481d5077de516758795084a860a15c747a889170`

Implementation commits or pull requests:

- `7c24ff09` — typed Linux launch dimensions, parent-ceiling validation, host feature probes, adapter dispatch, and documentation/guard updates.

## 1. Executive finding

M002's internal interface/probe slice is implemented. `ManagedProcessService` now validates a typed Linux launch request and selects only the existing private helper. The request models filesystem, network, process, syscall, and resource dimensions under a parent ceiling. A read-only probe reports host/kernel facts. The obtained projection records filesystem enforcement only from the private helper's status; generic helper success cannot imply that network, namespaces, seccomp, or resource bounds were applied.

No namespace backend is registered. Existing Landlock-only behavior, unrestricted legacy network behavior, and other platform paths remain unchanged. The closure is conditional on a macOS cross-target compile because the available Linux container lacks macOS SDK headers required by a dependency's C build. Windows GNU cross-compilation and Linux verification passed.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Typed launch dimensions and immutable parent ceiling | `src/security/sandbox/linux_launch.rs`; seven Linux launch contract tests | pass | Requests cover filesystem roots, network intent, process scope, syscall constraints, and resource budgets. Child requests cannot widen read/write grants or drop protected paths. |
| Canonical launch owner selects legacy path and refuses enhanced dimensions | `src/managed_process.rs`; adapter unit tests; managed-process tests | pass | Only the existing private helper is selectable; enhanced dimensions return unavailable before target launch. |
| Feature probes distinguish host facts from obtained child enforcement | Seven Linux launch tests and native host probe observations | pass | Namespace creation and cgroup delegation remain `Unknown` where read-only inspection cannot prove them. |
| Obtained status cannot overclaim enhanced dimensions | `LinuxObtainedEnforcement`; sandbox status decoder tests | pass | Filesystem backend/ABI/guarantees/limits come from the private status frame. Other dimensions remain `NotRequested`. |
| Test adapter and status fault handling | Linux adapter tests; `security::sandbox` and `managed_process` suites | pass | Existing private-channel tests cover malformed/duplicate/oversized status, failed setup, helper absence, target output spoofing, timeout, and cancellation. |
| Existing Linux filesystem enforcement remains intact | `sandbox_containment` and `sandbox_landlock` targets | pass | 13/13 containment tests and 3/3 native Landlock tests passed. |
| Static sandbox/ownership wiring remains valid | sandbox contract, sandbox policy-wiring, execution-ownership guards | pass | All three guards passed. |
| Repository quick verification | `scripts/verify.sh quick` | pass | All routine checks and workspace/all-targets check passed. |
| Windows platform compile | `cargo check -p codegg --lib --target x86_64-pc-windows-gnu --locked` | pass | Used the installed MinGW compiler; emitted existing platform-specific unused/dead-code warnings, no errors. |
| macOS platform compile | `cargo check -p codegg --lib --target x86_64-apple-darwin --locked` and retry with Clang target flags | blocked | Host `cc` does not accept Apple target flags; the Clang retry reaches `ring` but target SDK/sysroot headers are absent (`bits/libc-header-start.h`). No Rust diagnostic from this change was reported. |

## 3. Production implementation evidence

`src/security/sandbox/linux_launch.rs` defines the Linux-only request model, dimension demand, parent ceiling, production adapter, obtained-fact projection, and host feature probe. The production adapter accepts only the legacy private helper and returns an unavailable plan for enhanced process/network/syscall/resource requests. The probe avoids PATH lookup and process execution; a Bubblewrap candidate must be root-owned, executable, non-writable, and contained by root-owned non-writable parent directories. Probe results are diagnostics, not enforcement evidence.

`ManagedProcessService` creates and validates the legacy contract before constructing the existing helper argv, then maps the private helper result to typed obtained facts without changing the existing result DTO. The test-only adapter injects legacy, unsupported, and setup-failure outcomes. Existing private status framing remains the authority for the actual helper result.

## 4. Verification executed

### Commands run

```bash
CARGO_TARGET_DIR=/tmp/codegg-linux-execution-isolation-target cargo test -p codegg --lib security::sandbox::linux_launch -- --test-threads=1
CARGO_TARGET_DIR=/tmp/codegg-linux-execution-isolation-target cargo test -p codegg --lib security::sandbox -- --test-threads=1
CARGO_TARGET_DIR=/tmp/codegg-linux-execution-isolation-target cargo test -p codegg --lib managed_process -- --test-threads=1
CARGO_TARGET_DIR=/tmp/codegg-linux-execution-isolation-target cargo test --test sandbox_containment -- --test-threads=1
CARGO_TARGET_DIR=/tmp/codegg-linux-execution-isolation-target cargo test --test sandbox_landlock -- --test-threads=1
python3 scripts/check_sandbox_contract.py
python3 scripts/check_sandbox_policy_wiring.py
python3 scripts/check_execution_ownership.py
cargo fmt --all -- --check
git diff --check
CARGO_TARGET_DIR=/tmp/codegg-linux-execution-isolation-target scripts/verify.sh quick
CC_x86_64_pc_windows_gnu=x86_64-w64-mingw32-gcc AR_x86_64_pc_windows_gnu=x86_64-w64-mingw32-ar CARGO_TARGET_DIR=/tmp/codegg-linux-execution-isolation-target CARGO_BUILD_JOBS=2 cargo check -p codegg --lib --target x86_64-pc-windows-gnu --locked
CARGO_TARGET_DIR=/tmp/codegg-linux-execution-isolation-target CARGO_BUILD_JOBS=2 cargo check -p codegg --lib --target x86_64-apple-darwin --locked
CC_x86_64_apple_darwin=clang CFLAGS_x86_64_apple_darwin="-target x86_64-apple-darwin" CARGO_TARGET_DIR=/tmp/codegg-linux-execution-isolation-target CARGO_BUILD_JOBS=2 cargo check -p codegg --lib --target x86_64-apple-darwin --locked
```

### Results

Linux launch tests passed 7/7; full sandbox library tests passed 56/56; managed-process tests passed 19/19; containment integration passed 13/13; native Landlock passed 3/3. `scripts/verify.sh quick` passed. The Windows GNU library check passed. The macOS check was attempted with the host compiler and with Clang, but the target SDK headers are absent.

The read-only host probe inputs observed on this Linux host: `user.max_user_namespaces=28633`; user/mount/pid/net/ipc namespace handles exist, while namespace creation remains unknown; seccomp lists the `filter` action; `/proc/filesystems` lists `proc`; `/usr/bin/bwrap` is root-owned mode 755; Landlock integration tests pass; cgroup v2 exposes controllers but delegated ownership is unproven; AppArmor is present and SELinux is absent. No probe result is represented as a child enforcement claim.

## 5. Invariant review

- FullHost remains explicit and no launch profile or default was changed.
- Existing filesystem helper identity, private status pipe, bounded spec, cancellation, process-group cleanup, and scratch ownership remain in use.
- No namespace, network, seccomp, or resource backend was added to `BACKENDS`.
- A feature probe is not treated as evidence that a child obtained enforcement.
- Parent ceilings are checked before the helper is selected.
- macOS and Windows production code paths do not select the Linux adapter.

## 6. Failure and recovery review

Malformed or over-ceiling requests fail before helper/target launch. Enhanced dimensions produce an unavailable result and cannot fall back to an uncontained child. Existing private status tests reject malformed, duplicated, oversized, and contradictory frames. The managed-process suite covers helper absence, setup failure, output spoofing, timeout, cancellation, cleanup, and scratch removal. No durable receipt or shared resource was introduced.

## 7. Migration and compatibility review

No schema, protocol, CLI, config, or persisted policy changed. No migration is needed. The internal request is constructed from the existing `SandboxLaunchSpec`; public outcome meaning remains unchanged. Linux continues to use existing Landlock-only behavior; macOS Seatbelt and Windows paths retain their current implementations. macOS cross-compilation remains the named condition.

## 8. Security review

The production adapter does not execute Bubblewrap or use an untrusted PATH lookup. Its probe inspects fixed system paths and ownership/mode metadata only. Namespace creation, seccomp installation, cgroup delegation, LSM compatibility, and child setup are not inferred from the probe. The existing private helper status remains the only source of obtained filesystem facts. No credentials, environment, or command payload are added to logs.

## 9. Documentation and operations

- Updated `architecture/security.md` and `architecture/jobs.md` with launch ownership and probe limitations.
- Updated `docs/security-semantics.md` to clarify that host discovery does not mean child enforcement.
- Extended `scripts/check_sandbox_policy_wiring.py` to require launch-contract integration and prevent premature backend registration.
- `plans/implementation/linux-execution-isolation/002-composable-linux-launch-contract.md` is marked implemented.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | macOS cross-target compilation could not complete because the Linux environment lacks the SDK headers required by `ring`. | macOS compilation of unchanged paths is not evidenced on this host. | Run the macOS target check in a macOS or correctly provisioned cross-build environment; amend this record with the result. |
| high | No production Linux namespace, protected mount, process/IPC, seccomp, network, or cgroup enforcement backend exists in this slice. | Enhanced Linux containment is unavailable. | Keep ADR-0023 proposed; complete M003–M007 with native proof before backend promotion. |

## 11. Roadmap disposition

M002's interface/probe work is conditionally closed pending the macOS compile evidence above. This does not unblock M003: accepted ADR-0023 and native M003/M004 prerequisites remain outstanding. M007 and C002/C003 remain blocked on native dimensions and transport requirements. M001 remains blocked. No remaining implementation plan becomes eligible in this closure.

## 12. Registry updates

- Mark M002 conditionally closed and link this record in `plans/registry.md` and the Linux subsystem roadmap.
- Preserve M003–M008, C002/C003, and M001 blocked states.
- Do not register or advertise Bubblewrap/namespace enforcement until ADR-0023 is accepted and native enforcement is proven.
- The remaining condition is a macOS cross-target compile using a provisioned SDK/toolchain.
