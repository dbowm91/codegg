# Windows Native Sandbox Milestone 001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/windows-native-sandbox/001-launch-contract-and-fail-closed-authority.md`

Source subsystem roadmap:

- `plans/subsystems/windows-native-sandbox-roadmap.md#7-milestones`

Repository baseline reviewed: `a02cd9ade7ffca775efc1567669aae728ad9ff72`

Implementation commits:

- `1ccc86336a0ee35cc76233d4188701bcd12ce6ce` — fail closed before spawn when constrained filesystem containment is unavailable.

## 1. Executive finding

M001 is closed. A constrained Bash request on a host without a registered filesystem backend now returns an execution error before `ManagedProcessService` can spawn a direct child. The canonical process service independently rejects the compatibility `DegradedUncontained` request. Explicit `FullHost` remains the only unconstrained path. No Windows backend or Windows support claim was added.

The existing typed enforcement/status representation continues to separate the requested sandbox profile, filesystem enforcement, network enforcement, backend identity, and backend-reported guarantees/limits. Network remains explicitly unrestricted for the shipped filesystem-only backends. M002 may now implement the Windows launcher against this authority boundary.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Unavailable constrained Bash launch cannot silently run uncontained | `src/tool/bash/process.rs`; `tests/sandbox_policy_wiring.rs::unsupported_host_rejects_constrained_command_before_execution`; `scripts/check_sandbox_contract.py` self-test | pass | On this Seatbelt-capable host, the unsupported-host integration branch is skipped; the process-service rejection is directly tested below. |
| Canonical process service rejects the degraded compatibility request before spawn | `src/managed_process.rs`; `managed_process::tests::degraded_request_is_rejected_before_spawn` | pass | The test inspects launch preparation; no child is created. |
| Required launch status remains fail-closed | `managed_process::tests::required_request_with_helper_unavailable_still_fails_closed`; sandbox status decoder tests | pass | Missing or malformed private status cannot be interpreted as enforcement. |
| Filesystem and network status remain separate and backend claims remain truthful | `tests/sandbox_policy_wiring.rs`; `tests/sandbox_containment.rs`; backend guarantee/limit assertions | pass | macOS Seatbelt behavior was exercised live; network remains unrestricted. |
| Unix helper behavior remains intact | `tests/sandbox_containment.rs` on macOS 26.6.2 ARM64 | pass | All 8 live containment tests passed. |
| Ownership and unsupported-backend regressions are guarded | `scripts/check_execution_ownership.py`; `scripts/check_sandbox_contract.py --self-test` | pass | Guards and the new negative fixture passed. |
| Windows remains unavailable pending native implementation | `src/managed_process.rs` `cfg(not(unix))` required-launch rejection; Windows host unavailable in this environment | pass | No Windows execution or security qualification is claimed. |

## 3. Production implementation evidence

- `BashTool` returns a typed `ToolError::Execution` on the unavailable constrained path before building or submitting a managed-process request.
- `ManagedProcessService::prepare_launch_argv` rejects `SandboxRequest::DegradedUncontained`; callers cannot bypass the Bash decision and cause an uncontained child through the canonical service.
- `interpret_sandbox_status` also rejects the compatibility request, so it cannot be reported as a successful `Uncontained` outcome.
- The sandbox contract guard now detects both a direct degraded spawn in the managed service and a Bash branch that continues into managed execution.
- The architecture guide describes the fail-closed Bash behavior. No public protocol, storage, Unix helper frame, or backend identity changed.

## 4. Verification executed

### Commands run

The ARM64 test commands used Rust 1.89 explicitly because the default Homebrew x86_64 toolchain under Rosetta linked against incompatible native libraries.

```bash
rtk env RUSTC=/Users/davidbowman/.cargo/bin/rustc /Users/davidbowman/.cargo/bin/cargo +1.89-aarch64-apple-darwin test --target aarch64-apple-darwin -p codegg --lib security::sandbox
rtk env RUSTC=/Users/davidbowman/.cargo/bin/rustc /Users/davidbowman/.cargo/bin/cargo +1.89-aarch64-apple-darwin test --target aarch64-apple-darwin -p codegg --test sandbox_containment --test sandbox_policy_wiring
rtk env RUSTC=/Users/davidbowman/.cargo/bin/rustc /Users/davidbowman/.cargo/bin/cargo +1.89-aarch64-apple-darwin test --target aarch64-apple-darwin -p codegg --lib managed_process::tests::degraded_request_is_rejected_before_spawn
rtk bash scripts/verify.sh quick
rtk cargo fmt --all -- --check
rtk python3 scripts/check_sandbox_contract.py
rtk python3 scripts/check_sandbox_contract.py --self-test
rtk python3 scripts/check_sandbox_policy_wiring.py
rtk python3 scripts/check_execution_ownership.py
rtk git diff --check
```

### Results

- `security::sandbox`: 35 passed.
- `sandbox_containment`: 8 passed, including live Seatbelt child-process probes.
- `sandbox_policy_wiring`: 23 passed.
- Direct fail-closed process-service regression: 1 passed.
- `scripts/verify.sh quick`: passed, including workspace all-target cargo check.
- Formatting, sandbox contract guard and self-test, sandbox policy wiring guard, execution ownership guard, and diff whitespace check: passed.
- An initial test with Homebrew Rust 1.98.1 targeting x86_64 reached link and failed on missing native symbols / macOS deployment-target mismatch. Re-running with the repository MSRV Rust 1.89 ARM64 toolchain passed. This was a host toolchain/link configuration issue, not a source compile or test failure.
- No Windows host build or test was available or claimed.

## 5. Invariant review

- `ManagedProcessService` remains the only owner for finite process creation; the change removes a bypass rather than adding an owner.
- A constrained request cannot fall through to FullHost or direct execution when containment is unavailable.
- `FullHost` remains an explicit profile choice. Approval mode and Yolo do not rewrite the sandbox profile.
- Requested profile and obtained enforcement remain separate; filesystem success does not imply network isolation.
- Unix helper setup, status provenance, cancellation, and launch behavior were not altered.
- No status frame, backend ID, or stale capability can fabricate a successful launch.

## 6. Failure and recovery review

Unavailable containment now fails before target spawn and produces no process handle, status frame, or partial launch to recover. A helper/setup failure on a required launch remains terminal. Cancellation and concurrent launch ownership are unchanged. A daemon restart still re-probes capability through the existing backend registry.

## 7. Migration and compatibility review

No database, public protocol, status-frame, or migration change. `SandboxRequest::DegradedUncontained` and `SandboxExecutionOutcome::Uncontained` remain representable for source and historical compatibility, but the canonical launch path no longer creates an uncontained execution from that request.

## 8. Security review

The important security boundary is enforced twice: the Bash adapter rejects an unavailable backend, and the process owner refuses a degraded request even if a caller attempts to bypass Bash. The denied command is not included in the error; only the bounded backend reason is surfaced. No credentials or path contents are added to logs. This milestone does not provide filesystem containment on Windows, process-tree supervision there, or network isolation.

## 9. Documentation and operations

- Updated `architecture/security.md` to describe unavailable constrained Bash behavior.
- Extended `scripts/check_sandbox_contract.py` and its self-test to guard both launch boundaries.
- Updated the M001 implementation plan, roadmap, and registry after dependency audit.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | This host cannot provide Windows build or native process evidence. | Windows launch remains unavailable; no native security claim can be made. | M002 adds a supervised Win32 launcher and requires actual Windows process-tree tests. |

No unresolved high or medium finding remains in M001 scope.

## 11. Roadmap disposition

M001 is strictly closed. The launcher authority boundary is suitable for downstream implementation, but Windows remains unsupported until later milestones prove process ownership and actual filesystem enforcement. M002 is dependency-ready. M007 is separately dependency-ready as an optional research-only branch.

## 12. Registry updates

- M001 moved from active to closed and is recorded under Recently closed work.
- Dependency audit: M002's only hard dependency (M001 strict closure) is satisfied, so M002 moved from blocked to ready. M007's M001 interface dependency is satisfied, so the optional research plan moved from blocked to ready.
- M003 remains blocked on M002; M004 remains blocked on M003 and Windows security evidence; M005 remains blocked on M004 and an administrator-provisioned test host; M006 remains blocked on M004 (and M005 only for offline-network claims).
- No other registered plan declares M001 as a dependency.
