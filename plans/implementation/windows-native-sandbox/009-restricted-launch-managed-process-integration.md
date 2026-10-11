# Windows Native Sandbox Corrective C009 — Restricted Launch Through ManagedProcessService

Status: active

Repository baseline: `17ef5e09b46fa364a520c8dc8793ab866020674f`, branch `work/windows-native-sandbox-foundation`, 2026-10-11.

Corrects original implementation plan: `plans/implementation/windows-native-sandbox/004-filesystem-token-and-acl-enforcement.md`.

Original blocked closure evidence: `plans/closure/windows-native-sandbox/004-status.md` (immutable historical finding).

Source roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#m004-corrective-execution-sequence`.

Long-term requirements:
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`

Applicable ADRs: `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`; `plans/adrs/ADR-0016-cross-platform-sandbox-launch-authority.md`. No new architectural decision is authorized.

Primary class: infrastructure

## 1. Objective

Integrate a **restricted** AppContainer process into the existing managed-process launch/control boundary as an internal gated path, with real stdin/stdout/stderr, immutable sandbox spec, verified token/Job state, no inherited privileged handles, and typed launch outcomes. Do not advertise a complete production filesystem sandbox until C010/C011 prove rights and recovery.

## 2. Why this milestone is ready / dependency gate

Hard dependency: C008 accepted closure with successful hosted Rust token/Job launch and verified FFI lifetime. M001–M003 already closed. No privileged VM is needed to implement and test process-control integration on the hosted runner. ACL preparation and filesystem-rights assertions belong to C010's lease-owned fixture; C009 must not add ad hoc ACL mutation ahead of that ownership boundary.

## 3. Current implementation evidence and failed predecessor coverage

- `ManagedProcessService::run_inner` uses `tokio::process::Command` and `WindowsJob` for `SandboxRequest::Disabled`; `prepare_launch_argv` rejects `SandboxRequest::Required` for `cfg(not(unix))`.
- `src/managed_process/windows.rs` has a test-only `WindowsSuspendedAppContainerProcess` helper that is **not** connected to production outputs, deadline, status channel or cancellation.
- M004 historical blocked closure lists no production backend/status channel, no inherited handle or descendant adversarial proof, and no signed claim of enforcement; a passing C008 proof cannot close these gaps.

This is a corrective of the **blocked M004**, not a claim that `004-status.md` closed or that Windows filesystem sandboxing works. Its security regression tests must capture the evidence that the previous pass failed to obtain.

## 4. Invariants that must not regress

- There is one canonical process execution owner; no standalone sandbox tool bypass.
- A required constrained request must fail closed before first target instruction unless correct token, Job, status and requested permissions are established.
- Existing `SandboxExecutionPath`/`SandboxLaunchOutcome` separate requested and obtained guarantees; never claim network isolation.
- No secret/token/daemon handle leakage to child; timeout, cancellation and orphan cleanup cover descendants.

## 5. Scope

### In scope

- Refactor `ManagedProcessService` private child abstraction for two Windows launch paths: existing Tokio `Disabled/FullHost` and restricted `CreateProcessW(STARTUPINFOEXW)`, both owned by the service with explicit process/Job handles.
- Allow isolated **test-only** process-control execution with an empty filesystem-root policy; production remains disabled until C010 proves policy/ACL setup. Feature-gate or typed capability-gate this carefully.
- Implement bounded UTF-16 argv, cwd/env, stdio handle-list inheritance, controlled pipes, status frame, capture/output supervision, cancellation and exit accounting for restricted children.

### Explicitly out of scope

- ACL lease storage and recovery; global account provisioning; firewall/offline networking; backend activation prior to observed rights; changing approval mode and parent/child policy ceiling.

## 6. Required production changes

### Runtime
Extract a Windows owned-child adapter covering `WaitForSingleObject`/async integration, `GetExitCodeProcess`, Job termination and pipe EOF while keeping `ManagedProcessService` as caller. The child never resumes before token SID and Job ownership are checked. No detached subprocess or second execution service.

### Handle and status security
Use `STARTUPINFOEXW` `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` with `bInheritHandles=TRUE` only when narrow stdio inheritance is necessary; permit exactly standard handles, not a blanket inheritable set. Ensure handle lists and buffer ownership survive `CreateProcessW` as required. Bound private enforcement status channel and distinguish SetupError, ExecError and actual Enforced (the last not reachable for filesystem until C010/C011 capability gate).

### Policy/launch admission
Pass immutable `SandboxLaunchSpec`, approved roots, environment and workspace cwd; reject unsupported rights before launch. Expose an internal test-only allowed-root fixture seam with compile-time exclusion from release binaries and a guard preventing accidental always-on selection.

### Integration boundaries
Refresh static guard and ownership docs; do not duplicate Git/scheduler/agent permissions.

## 7. Ordered work packages

### WP-A — Unify owned Windows child lifecycle
Define one private Windows handle/pipes interface in `ManagedProcessService` for restricted vs unrestricted children. Prove timeout and output bounds remain identical.

### WP-B — Create confined process before resume
Use the repaired C008 attributes to create target suspended; query token and attach to Job; then attach only approved stdio, verified status and policy prerequisites; resume once.

### WP-C — Failure-injection matrix
Inject malformed status, failed Job assignment, pipe setup, token mismatch, duplicate handle inheritance, resume failure, cancellation before/during launch. Assert marker absence or controlled cleanup and no process tree leak.

### WP-D — End-to-end process-control fixture
Route a safe command via `ManagedProcessService` with no filesystem roots and observe real stdio, environment filtering, exit status, timeout, cancellation, and the explicit non-enforcement outcome. Do **not** mutate ACLs or set global Windows backend availability based on this fixture. C010 WP-D owns the follow-up fixture that combines this service path with journaled ACL leases and allowed/denied roots.

## 8. Failure, cancellation, restart, and contention semantics

Race-free lifecycle of suspend/job/verify/resume must survive cancellation, deadline and startup failure. Child may exit before async waiter attaches; owned process HANDLE remains authoritative. Parent death closes Job handle. If target quits and descendant keeps stdio open, Job closure precedes waiting for pipe EOF. Concurrent roots have separate package names, Jobs and handles. A valid sandbox status cannot be spoofed over stdout/stderr.

## 9. Compatibility and migration

No persistent migration. Unix helper transport and inherited file-descriptor status stay unchanged. Add private Windows status wire version only if necessary, with bounds and explicit backend id. No change to `FullHost` configuration or automatic escalation.

## 10. Required tests

- Native hosted integration: test-only restricted `ManagedProcessService` command receives real stdin/stderr/stdout and exit status with an empty filesystem-root policy; timeout and cancellation terminate the owned Job.
- Security negative: arbitrary handle inheritance, daemon pipe, job escape attempt, forged/missing status, token mismatch, credential env leaks, indirect child command.
- Timeout/cancellation: child/grandchild terminated, no Job handle inherited, runner remains usable.
- Regression: `WindowsJob` unconstrained suite unchanged, Unix sandbox tests/guards unaffected.
- Native tests must use same version/revision and record whether privileged VM tests skipped.

## 11. Required verification commands

On Windows:

`cargo test --locked -p codegg --lib managed_process::windows::tests::appcontainer_ -- --nocapture`
`cargo test --locked -p codegg --test windows_process_launcher -- --nocapture --test-threads=1`
`cargo test --locked -p codegg --test windows_restricted_managed_process -- --nocapture` (new target)

Cross-host:

`python3 scripts/check_execution_ownership.py`
`python3 scripts/check_sandbox_contract.py`
`cargo fmt --all -- --check`
`bash scripts/verify.sh quick`

The closure must record exact versions, run IDs, skipped coverage and failures. Do not mark tests passed if the hosted runner has no rights to execute them. New test filenames are prescribed as targets to create, not files assumed to exist today.

## 12. Documentation updates

`architecture/process-tool-execution-ownership.md`, `architecture/security.md`, `architecture/testing.md`, C009 closure record and registry/roadmap status.

## 13. Acceptance criteria

- Production code has one integrated restricted child-process lifecycle; a test-only process policy with no filesystem roots executes end-to-end through canonical service. C010 must exercise allowed/denied roots under lease ownership before any filesystem guarantee is considered.
- Token SID and Job verified before resume; stdout/stderr and exit/cancellation match managed-process semantics.
- Constrained production `ReadOnly`/`WorkspaceWrite` remain unavailable until C010/C011 complete ACL and read-denial proof; no backend falsely advertises Enforced.

## 14. Stop conditions

Stop if integration bypasses `ManagedProcessService`, cannot safely control stdio inheritance, breaks fail-before-resume ordering or needs new daemon privilege. If reliable process creation depends on a target-specific .exe/activation method, record OS limitations and do not silently fall back.

## 15. Closure evidence required

Live service-level Windows run, token/Job pre-resume trace, status and pipe ownership evidence, descendant cancellation, failure-injection table, production capability gate/guard, Unix regression, C010 unblock.

Do not edit the historical `plans/closure/windows-native-sandbox/004-status.md` to turn its failures into successes. Create `plans/closure/windows-native-sandbox/009-status.md` and make a later M004 requalification/closure decision from aggregated evidence.

## 16. Handoff notes

See `src/managed_process.rs` launch, `src/managed_process/windows.rs` and `src/security/sandbox.rs`; the existence of an AppContainer child is not proof its file policy matches the requested `SandboxProfile`.
