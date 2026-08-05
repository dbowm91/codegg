# Runtime Safety, Resource Control, and Footprint Milestone 001 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/runtime-safety-resource-footprint/001-landlock-and-sandbox-contract-correction.md`

Source subsystem roadmap:

- `plans/subsystems/runtime-safety-resource-footprint-roadmap.md`

Repository baseline reviewed: `f69d243`

Implementation commits:

- `f69d243` — replace handwritten Landlock enforcement with the private maintained-crate helper and correct Python/Bash sandbox contracts

## 1. Executive finding

M001's production implementation is complete and the unsafe fail-open paths are corrected. The custom Landlock syscalls, filesystem heuristics, ignored rule failures, zero-access pseudo-deny rules, and Python `pre_exec` policy construction are gone. A private one-shot helper now constructs and applies the complete ABI-aware policy in a normal child process, requires full enforcement and `no_new_privs`, reports the effective ABI, and only then `exec`s the target. Python Analyze/Verify use read-only roots; Transform uses workspace write roots. Bash uses the same child-only helper and cannot restrict the daemon process.

Strict closure is conditional only because this Darwin workspace cannot execute the required supported-Linux enforcement run. The Linux integration fixture and non-Linux behavior are present and verified; no supported-Linux ABI result is claimed here.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Maintained Landlock backend with ABI-aware rights | `landlock` 0.4.7 dependency; `src/security/sandbox.rs`; source guard | pass | No handwritten Landlock syscall symbols remain in governed production sources. |
| Authoritative availability path | `probe_landlock()` uses maintained crate hard-requirement ruleset creation | pass | Replaces `/proc` and securityfs heuristics; restriction success is verified separately by the helper. |
| All rules must succeed | `apply_landlock()` and `tests/sandbox_landlock.rs` setup-failure fixture | pass | Missing paths and add/restrict failures stop before target exec. |
| No pseudo-deny rules | source review and `check_sandbox_contract.py` | pass | Landlock allow-list semantics provide denial outside handled roots. |
| Child-only setup; daemon remains unrestricted | helper binary; Bash/Python call paths; parent-enforcement regression test | pass | `SandboxConfig::enforce()` refuses enabled parent restriction. |
| Typed outcome and failure distinction | `SandboxOutcome`, `SandboxFailureKind`, `PythonPolicyDecision::outcome`; marker parser tests | pass | Enforced ABI, fallback, disabled, unavailable, policy, setup, and helper states are typed. |
| Python read-only vs Transform write profile | `build_landlock_paths()` and Python focused tests | pass | Transform no longer hard-codes read-only Landlock mode. |
| Required-mode fail-closed behavior | helper reserved failure status and Linux setup-failure fixture | pass | A helper setup failure exits before target execution. |
| Read-only, workspace-write, and outside-root enforcement | Linux integration fixture | partial | Fixture covers all cases, but this host is Darwin and cannot run the Linux branch. |
| Non-Linux behavior | `cargo test --test sandbox_landlock` | pass | One non-Linux unavailable test passed on Darwin. |
| No migration or public daemon protocol change | source and manifest review | pass | Only ephemeral local helper JSON is added. |
| Focused and quick verification | commands in §4 | pass | Required local checks passed. |

## 3. Production implementation evidence

- `src/security/sandbox.rs` now owns the typed bounded launch specification, maintained-crate probe, ABI-aware ruleset application, helper discovery, canonical path validation, and child-only compatibility boundary.
- `src/bin/codegg-sandbox-helper.rs` is a private one-shot executable. It accepts only a bounded JSON spec, probes Landlock, applies every rule, requires `FullyEnforced` and `no_new_privs`, emits a reserved outcome marker, and replaces itself with the target.
- `src/python_script/executor.rs` serializes policy before launch, sends Linux Landlock requests through the helper, cleans reserved markers, records typed outcomes, and maps setup failures to spawn failure without running user Python.
- `src/tool/bash.rs` no longer invokes sandbox enforcement in the daemon. Enabled Bash sandbox requests force the raw-shell path through the helper.
- `src/python_script/types.rs` and projection code expose typed sandbox outcomes without changing the daemon protocol or durable schema.
- `scripts/check_sandbox_contract.py` prevents reintroduction of direct Landlock syscall symbols or sandbox `pre_exec` policy construction and is included in quick verification.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
cargo check -p codegg --all-targets --locked
cargo clippy -p codegg --all-targets --locked -- -D warnings
cargo test -p codegg sandbox --lib -- --test-threads=1
cargo test --test sandbox_landlock -- --test-threads=1
cargo test --test python_sandbox_adversarial -- --test-threads=1
python3 scripts/check_sandbox_contract.py
scripts/verify.sh quick
```

### Results

- Formatting, package check, clippy, sandbox unit tests, adversarial Python tests, and quick verification passed.
- Sandbox unit target: 54 passed.
- Non-Linux `sandbox_landlock` target: 1 passed; it recorded Landlock unavailable on Darwin.
- The Linux integration tests include precise runtime skip reasons for unsupported kernels, but were not executable on this host.
- `cargo check --target x86_64-unknown-linux-gnu --tests --locked` was attempted and blocked by the host's missing Linux linker/C toolchain and OpenSSL sysroot; it is not represented as Linux evidence.
- A broad local `cargo test -p codegg --lib` attempt aborted in an existing macOS linker/test-process resource path; the focused suites and canonical quick verification remained green.

## 5. Invariant review

- The daemon is never restricted: all actual Landlock calls are in the helper executable, and the old parent-side Bash call now fails closed.
- Child authority is bounded by canonicalized read/write roots and target/runtime paths; outside handled rights remain denied by allow-list semantics.
- Required setup failure cannot reach `exec`; the helper reserves exit status 125 and emits a typed marker.
- Partial rule construction cannot be reported as enforcement; every path is opened and added with `?`, and `FullyEnforced` plus `no_new_privs` is required.
- No complex Rust policy construction remains in a sandbox `pre_exec` closure.
- Unsupported hosts use an explicit portable fallback for Python policy resolution and never claim Landlock.

## 6. Failure and recovery review

- Duplicate or stale helper specs are bounded to 64 KiB and are ephemeral; parent cleanup runs after completion and failure paths remove the generated Python spec.
- Malformed, missing, oversized, unavailable, setup-failed, and exec-failed helper requests exit through the reserved helper failure path; target output is not confused with setup diagnostics.
- Timeout/cancellation remains owned by the existing parent `tokio::process::Command` and `kill_on_drop` path; M002 retains ownership of broader output/descendant lifecycle correction.
- Missing required paths and rule-add errors fail before restriction/exec; no partial policy is accepted.

## 7. Migration and compatibility review

- No database, RunStore, durable asset, or daemon network protocol migration is required.
- Existing `SandboxConfig` builders remain available. Enabled callers now use `launch_spec()` and the child helper; direct `enforce()` on an enabled config refuses unsafe parent restriction.
- Non-Linux builds compile the helper as an unavailable compatibility binary and report the portable Python outcome.
- The `landlock` dependency is Linux target-gated and locked at 0.4.7.

## 8. Security review

Independent second-pass review of the implementation diff, governed source paths, helper boundary, failure markers, allow-list construction, and focused negative tests found no unresolved critical, high, or medium defect.

- Path policy is canonicalized before helper launch; missing required paths are errors.
- Runtime roots are explicit and no longer rely on host filesystem feature files for availability.
- No secret material is placed in the helper spec; it contains only target argv and filesystem paths.
- The helper never becomes a daemon or network service and does not broaden scheduler or tool authority.
- Linux enforcement evidence remains outstanding and is the sole conditional-closure item.

## 9. Documentation and operations

- Updated `architecture/security.md` and `architecture/python_scripting.md` with the child-only helper, allow-list, ABI, fallback, and failure contract.
- Added `scripts/check_sandbox_contract.py` to `scripts/verify.sh quick`.
- Supported-host maintainer command: `cargo test --test sandbox_landlock -- --test-threads=1` after building all targets on a Landlock-capable Linux host. Record kernel version, effective ABI, and all fixture outcomes.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Supported-Linux enforcement fixture was not runnable on the Darwin host | Strict security evidence is incomplete; code correctness is not disproven | Run the existing fixture on a Landlock-capable Linux host and attach kernel/ABI/output evidence before changing this record to `closed`. |

## 11. Roadmap disposition

M001 is conditionally closed. Its production sandbox request/outcome contract is implemented and reviewable, but M002 is not promoted because the registry defines accepted M001 closure as a hard dependency and supported-Linux enforcement evidence remains outstanding. M004 and M005 remain dependency-ready and can proceed independently.

## 12. Registry updates

- Mark the implementation plan `implemented`.
- Remove M001 from the dependency-ready table and record it as conditionally closed.
- Keep M002 blocked on strict M001 closure evidence; do not promote it implicitly.
- Leave M003 and M006–M008 blocked on their existing dependencies.
- Keep the runtime-safety roadmap `active` with M001 conditional and M004/M005 ready.
