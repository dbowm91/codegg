# Sandbox Containment Post-Closure Corrective C001 — Closure

Status: closed

Source plan: `plans/implementation/sandbox-containment-post-closure-corrective/001-macos-python-and-strict-launch-contract.md`

Implementation commit: `f4560665cbfe20ee7d478a56a114bdb3f164f8dc`

## 1. Executive finding

The shared launch contract is implemented and qualified on Linux and native Apple Silicon macOS. Python selects Seatbelt on macOS and Landlock on Linux; Analyze and Verify cannot persist workspace writes, Transform can persist only to the approved workspace, and required launches remain fail-closed. The real Seatbelt helper reports the applied backend and its limitations. C001 is closed; broader read, network, descriptor, and cross-process limits remain in C004's contract.

## 2. Requirement evidence

| Requirement | Evidence | Disposition |
|---|---|---|
| Python selects registered platform backends | `resolve_enforcement_backend_for_capability` and injected capability unit tests in `src/python_script/sandbox.rs`; 44 focused tests passed | Complete for shared selection logic |
| Required constrained Python launch does not run without an OS backend | `src/python_script/executor.rs` records unavailable and returns before script creation/launch; policy wiring guard checks this path | Complete |
| Constrained Bash does not fall back to raw host execution | `SandboxExecutionPath::Unavailable` fails before managed launch; degraded outcome/launch variants removed; marker absence is checked in managed-process tests | Complete |
| Helper failure/status failure remains fail-closed | managed process regression passes with target marker absent on missing helper status | Complete |
| Native macOS Python Analyze/Verify/Transform under Seatbelt | Hosted Apple Silicon run `38166386985`, commit `98f96a99`; `python_script::executor::tests` — 22 passed, including Analyze/Verify write denial and Transform workspace-only writes | Complete |
| Native helper applies Seatbelt before target execution and reports actual identity | Hosted run `38166386985`; `sandbox_containment` — 15 passed, actual status `Enforced { backend: seatbelt, abi: None }`; malformed and rejected profiles leave target markers absent | Complete |
| Required-launch scratch is private and cleaned after reap on macOS | Hosted run `38166386985`; exact `constrained_launch_gets_private_tmpdir_removed_after_reap_on_macos` — 1 passed; canonical temp-root path, mode `0700`, and cleanup verified | Complete |

## 3. Verification executed

Local Linux evidence was collected on `6.8.0-146-generic`; native qualification ran on hosted Apple Silicon as recorded below.

- `rtk proxy cargo test -p codegg --lib python_script::sandbox` — 44 passed.
- `rtk proxy cargo test -p codegg --lib managed_process::tests::missing_private_status_fails_closed_with_test_only_helper_injection` — 1 passed; target marker absent.
- `rtk proxy cargo test -p codegg --test sandbox_policy_wiring -- --test-threads=1` — 22 passed.
- `rtk python3 scripts/check_sandbox_policy_wiring.py` — passed.
- `rtk python3 scripts/check_sandbox_contract.py` — passed.
- `rtk cargo fmt --all -- --check` and `rtk git diff --check` — passed.
- `rtk scripts/verify.sh quick` — passed, including workspace all-target `cargo check`.

- Hosted native run `38166386985` on commit `98f96a99`: `macos-15-arm64`, macOS `15.7.9` build `24G830`, Darwin `24.6.0`, `RELEASE_ARM64_VMAPPLE arm64`; `/usr/bin/sandbox-exec` was executable.
- `cargo test -p codegg --test sandbox_containment -- --test-threads=1 --nocapture` — 15 passed. The real helper reported Seatbelt enforcement, broad reads, no network isolation, usable inherited pre-opened descriptors, and the documented Apple Events/Launch Services and privilege-transition limits.
- `cargo test -p codegg --lib python_script::executor::tests -- --test-threads=1 --nocapture` — 22 passed.
- `cargo test -p codegg --lib managed_process::tests::constrained_launch_gets_private_tmpdir_removed_after_reap_on_macos -- --test-threads=1 --nocapture` — 1 passed.
- `cargo test -p codegg --lib managed_process::tests::timeout_and_cancellation_kill_the_process_group -- --test-threads=1 --nocapture` — 1 passed.
- `cargo test -p codegg --lib managed_process::tests::missing_private_status_fails_closed_with_test_only_helper_injection -- --test-threads=1 --nocapture` — 1 passed.
- `cargo test -p codegg --lib managed_process::tests::hardlinked_denied_path_fails_before_target_launch -- --test-threads=1 --nocapture` — 1 passed.
- `python3 scripts/check_sandbox_contract.py` and `python3 scripts/check_sandbox_policy_wiring.py` — passed in the hosted run. Local `scripts/verify.sh quick` and the focused Linux commands listed above also passed.

## 4. Invariant, failure, compatibility, and security review

The daemon remains unrestricted and the existing `ManagedProcessService` remains the sole target launch/supervision owner. A constrained profile cannot become `FullHost` after capability failure. `FullHost` remains the explicit opt-out. The change intentionally rejects formerly degraded uncontained constrained work. No storage or protocol migration was introduced. Network isolation, inherited descriptors, subprocess/IPC channels, and Seatbelt-specific limits are not claimed as solved.

## 5. Unresolved findings

No C001-specific findings remain open. Seatbelt's broad-read and non-filesystem limits are explicit and are tracked in C004, rather than represented as C001 failures.

## 6. Roadmap and registry disposition

C001 is closed on Linux and native macOS evidence. C003's shared-launch interface and C002's ABI requirement are both satisfied. C004's native qualification gate is discharged; its own residual IPC and privilege evidence is recorded in `004-status.md`. Historical runtime-safety closure records remain unchanged.
