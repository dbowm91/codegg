# Sandbox Containment Post-Closure Corrective C001 — Closure

Status: conditionally closed

Source plan: `plans/implementation/sandbox-containment-post-closure-corrective/001-macos-python-and-strict-launch-contract.md`

Implementation commit: `f4560665cbfe20ee7d478a56a114bdb3f164f8dc`

## 1. Executive finding

The shared launch contract is implemented. Python backend selection is no longer Linux-only, unavailable constrained execution is refused before target code, and the direct degraded Bash/managed-process route has been removed. The Linux helper and policy paths pass focused and quick verification. This is conditionally closed because this Linux host cannot qualify the native Darwin Seatbelt helper; no macOS execution evidence is claimed.

## 2. Requirement evidence

| Requirement | Evidence | Disposition |
|---|---|---|
| Python selects registered platform backends | `resolve_enforcement_backend_for_capability` and injected capability unit tests in `src/python_script/sandbox.rs`; 44 focused tests passed | Complete for shared selection logic |
| Required constrained Python launch does not run without an OS backend | `src/python_script/executor.rs` records unavailable and returns before script creation/launch; policy wiring guard checks this path | Complete |
| Constrained Bash does not fall back to raw host execution | `SandboxExecutionPath::Unavailable` fails before managed launch; degraded outcome/launch variants removed; marker absence is checked in managed-process tests | Complete |
| Helper failure/status failure remains fail-closed | managed process regression passes with target marker absent on missing helper status | Complete |
| Native macOS Python Analyze/Verify/Transform under Seatbelt | No Darwin host available in this environment | Outstanding qualification |

## 3. Verification executed

All results are local Linux evidence on `6.8.0-146-generic`.

- `rtk proxy cargo test -p codegg --lib python_script::sandbox` — 44 passed.
- `rtk proxy cargo test -p codegg --lib managed_process::tests::missing_private_status_fails_closed_with_test_only_helper_injection` — 1 passed; target marker absent.
- `rtk proxy cargo test -p codegg --test sandbox_policy_wiring -- --test-threads=1` — 22 passed.
- `rtk python3 scripts/check_sandbox_policy_wiring.py` — passed.
- `rtk python3 scripts/check_sandbox_contract.py` — passed.
- `rtk cargo fmt --all -- --check` and `rtk git diff --check` — passed.
- `rtk scripts/verify.sh quick` — passed, including workspace all-target `cargo check`.

Native macOS qualification still required: record the exact Darwin version; run helper-backed Analyze, Verify, and Transform; prove Analyze/Verify writes fail, Transform writes only to its approved workspace path, and missing/rejected Seatbelt application leaves the target marker absent; capture the helper's actual enforcement status.

## 4. Invariant, failure, compatibility, and security review

The daemon remains unrestricted and the existing `ManagedProcessService` remains the sole target launch/supervision owner. A constrained profile cannot become `FullHost` after capability failure. `FullHost` remains the explicit opt-out. The change intentionally rejects formerly degraded uncontained constrained work. No storage or protocol migration was introduced. Network isolation, inherited descriptors, subprocess/IPC channels, and Seatbelt-specific limits are not claimed as solved.

## 5. Unresolved findings

- **Medium — native macOS qualification absent.** The production selection and fail-closed path are implemented, but native Seatbelt serviceability and all three Python modes need the evidence listed above. This is an environment condition, not a claim of parity.
- **Low — hosted CI not observed.** Local quick verification passed.

## 6. Roadmap and registry disposition

C001 is conditionally closed. The backend-neutral Python selection and strict launch interface are stable for C003. Registry dependency audit: C003's other hard dependency, C002, closed in the same closure update, so C003 is moved to active. C004 remains blocked on C003 and native C001 qualification; its native evidence requirement is not discharged by this record. Historical runtime-safety closure records remain unchanged.
