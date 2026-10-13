# Sandbox Containment Post-Closure Corrective C001 — macOS Python Seatbelt and Strict Launch Contract

Status: implemented (closed; Linux and native Apple Silicon qualification complete)

Repository baseline: `main` `8e9d8b01e5c229715c8e4dea929e050b391e252c` (2026-10-10).

Source roadmap: `plans/subsystems/sandbox-containment-post-closure-corrective-roadmap.md#7-milestones-and-useroperator-exits`

Long-term authority: `plans/000-long-term-specification.md`, `001-terminology-and-domain-model.md`, `002-long-term-roadmap.md`, `003-planning-process.md`; ADR-0004; `architecture/permission.md`, `architecture/security.md`.

Predecessors: runtime-safety M001/C001 and their historical closure records `plans/closure/runtime-safety-resource-footprint/{001,009,010}-status.md`. Their former dispositions remain historical and any outstanding Linux evidence is separate.

Primary class: invariant (mandatory filesystem-enforcement and authorization truth); capability (macOS Python)

## 1. Objective

Make Python Analyze/Verify/Transform on macOS actually use the installed Seatbelt backend; unify launch-state selection so a constrained operation that *requires* OS containment does not execute uncontained because the capability probe failed; and keep the operator-facing requested/obtained enforcement and network limitations truthful. Do this through the existing `ManagedProcessService` and one-shot helper, not a second launch mechanism.

## 2. Readiness

Registered Seatbelt and Landlock backends, common `SandboxLaunchSpec`, helper protocol, `SandboxRequest`, `SandboxExecutionOutcome`, M005 approval profiles, and existing Python executor all exist. No unclosed hard implementation dependency. Existing historical Linux qualification remains operational, not a blocker to implementing the macOS correction.

## 3. Evidence and defect reproduction

- `src/python_script/sandbox.rs::resolve_enforcement_backend` wraps registry selection in `#[cfg(target_os = "linux")]`. On Darwin the same code selects `PortableFallback` even when `platform_sandbox_capability()` advertises `BackendId::SEATBELT`.
- `src/python_script/executor.rs` only constructs `SandboxLaunchSpec` when `SandboxBackend::Os`; fallback then sets `SandboxRequest::Disabled`, and its result may call that a portable fallback despite Seatbelt being serviceable.
- `src/security/sandbox.rs::sandbox_execution_path` chooses `DegradedUncontained` on no backend; `src/tool/bash/process.rs::execute_via_raw_shell` maps it into direct target execution; `src/managed_process.rs` intentionally runs this variant without the helper. This is distinguishable in audit, but a constrained required request can still mutate host state without containment.
- The `Required` helper path separately treats mid-launch failure as terminal and should remain so.
- `src/python_script/executor.rs` already has `build_landlock_paths` whose naming and semantics should be reconciled as the shared policy is applied; do not introduce macOS-only ad hoc read and write sets.
- `tests/sandbox_containment.rs` proves helper behavior but not Python end-to-end on macOS.
- A backend's lack of network isolation is a real limit; Python AST screening is not OS network isolation and must not be reported as such.

## 4. Invariants

A constrained `SandboxProfile` never becomes `FullHost` based on probe failure, and `Yolo`/automatic approval never widens the authority ceiling. OS sandbox application must precede executing the target; a missing helper, malformed spec, rejected Seatbelt profile, status-channel failure or conflicting capabilities is terminal for `Required`. No parent/daemon confinement, detached child, stdout marker spoofing or unbounded output. Python mode transforms only access approved workspace/scratch paths; Analyze/Verify cannot write ordinary persistent files. No promise that filesystem containment blocks network traffic, subprocesses or IPC unless separately enforced.

## 5. Scope

**In scope:** Python macOS registry-based backend selection; real helper-backed launch spec; per-mode write-root mapping and warnings; fail-closed default for required constrained Bash launches; explicit authorized degraded behavior only where permitted by an existing typed authority contract; operator reporting, focused tests and static guard.

**Out of scope:** Landlock ABI upgrades (C002); global read/write allowance redesign and overlapping deny rules (C003); network/IPC isolation (C004); general scheduler refactor or changing `FullHost` opt-in policy.

## 6. Required production changes

**Platform-neutral backend resolution:** remove the Linux-only selection gate, call `platform_sandbox_capability` for Unix supported backends, map `Seatbelt` to `SandboxBackend::Os`, and represent unavailable/failed setup distinctly. Reserve `PortableFallback` for a consciously authorized best-effort mode, not as a silent default just because OS != Linux. Keep separate requested and obtained backend in `PythonPolicyDecision`; do not fabricate `abi` for Seatbelt.

**Unified launch requirement:** make `sandbox_execution_path` and Bash conversion express the existing `Required` semantics consistently. For constrained profiles, failure to obtain required containment yields a typed refusal before user command launch, rather than `DegradedUncontained` execution. If legacy best-effort support is necessary, it must have an explicitly granted authority path from `ApprovalRouter` / existing policy snapshot and be recorded as uncontained; never infer consent from probe failure or permission mode. Preserve `FullHost` as deliberate and independently confirmed no-containment.

**Python:** reuse helper-backed `SandboxLaunchSpec` for all supported Unix backends; ensure executable, cwd, temporary script, interpreter libraries and per-mode roots can be resolved. Reuse existing backend-neutral path helper where appropriate; do not silently substitute a different security policy for Python. On helper setup failure classify spawn/sandbox error accurately with no target execution; only report `SandboxOutcome::Enforced` after actual status confirmation. Eliminate contradictory warnings like "OS sandbox unavailable" on macOS with Seatbelt capability.

**Operator/audit:** ensure CLI/TUI/session policy and Python projection expose actual backend/limits and `network=unrestricted` where appropriate. Do not log source code, generated confidential paths, or SBPL profiles in normal logs. Treat a capability probe as advisory until the helper applies the real policy.

**Compatibility:** only safety-tighten unavailable-host constrained execution; preserve existing request payloads and public profile names when possible. No DB migration. If an explicit, documented best-effort public behavior currently guarantees uncontained execution and cannot be changed without broader authority redesign, stop, document the conflict and propose an ADR rather than silently changing the guarantee.

## 7. Ordered work packages

A. Add tests demonstrating Darwin's current `PortableFallback` selection and an uncontained command marker executed after a simulated unavailable backend. Pin expected protected outcomes before modifications.

B. Change Python backend capability negotiation and build shared helper specs for Analyze/Verify/Transform; propagate backend identity/limits through policy decision and managed-process result.

C. Correct unavailable-backend execution authorization at the Bash tool boundary; ensure `Required` remains required after a probe/setup race, status-protocol mismatch or cancellation.

D. Validate environment and temporary script permissions; run normal Python, shell, cargo smoke via the helper (macOS where possible) without suppressing security failures. Keep the daemon unrestricted.

E. Update `architecture/security.md`, `architecture/python_scripting.md`, `docs/cli.md` if operator behavior changes, and `scripts/check_sandbox_policy_wiring.py` to guard backend-neutral Python selection + no direct degraded constrained launch.

## 8. Failure, cancellation, restart, contention

Failed application, helper unavailable/removed after probe, invalid profile and truncated/private status: typed terminal error; target-side marker absent. Cancellation and timeout terminate supervised descendants, not the daemon. Sandbox selection and authorized roots are captured per execution/turn and cannot be enlarged by concurrent preference changes. No persistent migration or restart replay of failed command. No fallback from the supervised helper to a raw shell.

## 9. Tests

Unit: Python backend selection on Darwin and Linux; modes and effective write roots; unavailable-platform decision; accurate warnings and backend IDs; request vs obtained status; no default `DegradedUncontained` for `Required`.

Integration: macOS helper executes Python in each mode under Seatbelt; Analyze/Verify workspace mutation attempt fails, Transform approved mutation succeeds and outside fails; malformed Seatbelt application and missing helper yield no marker; status channel cannot be forged via stdout/stderr. On Linux confirm Python Landlock behavior unchanged. Use feature/OS gates with explicit skips on unsupported hosts; a skip is not macOS qualification.

Security: test profile toggles, `Yolo`, unavailable host, probe/application race and old best-effort configuration. Negatives include shell command under required sandbox and denied paths under Python.

No broad restart/DB scenario is necessary; demonstrate cancellation/timeout leaves no orphan and review process code for inherited descriptors.

## 10. Verification

```bash
cargo fmt --all -- --check
cargo test -p codegg --lib python_script::sandbox
cargo test -p codegg --lib python_script::executor
cargo test -p codegg --test sandbox_containment -- --test-threads=1
cargo test -p codegg --test sandbox_policy_wiring
python3 scripts/check_sandbox_contract.py
python3 scripts/check_sandbox_policy_wiring.py
scripts/verify.sh quick
```

Use actual available test names and target features; run host-native Darwin fixture on one Seatbelt-capable macOS host and record OS version, backend identity, effective outcome and pass/fail. A Linux run is desirable for cross-regression. Do not add a second permanent CI matrix.

## 11. Acceptance and stop criteria

Accept only if macOS Python reaches `Enforced { backend: seatbelt }` through the real helper, three modes enforce the intended mutation policy, Linux Python remains enforced, and required Bash/Python commands cannot run if containment is unavailable or setup fails. Operator diagnostics never confuse OS capability with actually obtained isolation.

Stop for a conflict between mandatory fail-closed semantics and an accepted external API contract, a missing helper protocol capability requiring a new public protocol, a demonstrably unfixable platform restriction, or a new trusted authority source needed outside the existing approval model. Do not mark closed from compile-only evidence.

## 12. Closure evidence

Create `plans/closure/sandbox-containment-post-closure-corrective/001-status.md` with commit/PR, regression-to-fix matrix, Darwin OS version and real helper outcome or explicit conditional blocker, Linux regression result, test/guard/quick commands, failure and cancellation evidence, security review findings by severity, and readiness assessment for C003's shared-policy interface. Update roadmap and registry; do not edit predecessor closure history.
