# Windows Native Sandbox Corrective C008 — AppContainer FFI Attribute Lifetime and Host Proof

Status: ready for handoff

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

Primary class: invariant

## 1. Objective

Repair the likely dangling pointer passed to `UpdateProcThreadAttribute` and obtain a reproducible native Rust `CreateProcessW` AppContainer token/Job launch proof. No production sandbox activation in this pass.

## 2. Why this milestone is ready / dependency gate

M001–M003 have strictly closed. M004 is blocked with hosted errors. This corrective has no hard predecessor other than accepted ADR-0016 and the existing test-only Windows fixture; hosted Windows Server 2025 runners are sufficient to run it. Source inspection shows an immediate, reviewable FFI lifetime violation.

## 3. Current implementation evidence and failed predecessor coverage

- In `src/managed_process/windows.rs`, `WindowsProcessAttributes::set_appcontainer` stores a pointer to a **stack-local** `SECURITY_CAPABILITIES` in the process attribute list; `CreateProcessW` runs later, after the local goes out of scope. Microsoft's `UpdateProcThreadAttribute` `lpValue` contract requires that memory to persist until `DeleteProcThreadAttributeList`. The package SID also needs to outlive all attributes/launches that refer to it.
- The C# fixture `research/windows-native-sandbox/appcontainer-access-proof.ps1` keeps `SecurityCapabilities` backing allocated through `CreateProcess` and previously passed in hosted runs `38162151380`, `38164279173`, `38166050591`.
- The Rust proof failed on `38169853082` and `38173319891` with Win32 error 87; changing cwd and flags did not help. This lifetime defect is a high-confidence *candidate cause*, not a confirmed sole cause.

This is a corrective of the **blocked M004**, not a claim that `004-status.md` closed or that Windows filesystem sandboxing works. Its security regression tests must capture the evidence that the previous pass failed to obtain.

## 4. Invariants that must not regress

- AppContainer SID and all pointers referenced by `PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES` remain stable until after `DeleteProcThreadAttributeList`.
- `STARTUPINFOEXW` and `SECURITY_CAPABILITIES` are ABI-correct, initialized, and held through process creation.
- Suspended target cannot run until token SID and Job ownership are verified; injected failure does not resume or leak.
- Missing/partial backend never executes a constrained request; Unix Landlock/Seatbelt behavior unchanged.

## 5. Scope

### In scope

- Pin the `SECURITY_CAPABILITIES` value in an owned stable allocation (e.g. `Box<SECURITY_CAPABILITIES>`) held by `WindowsProcessAttributes`; keep package SID and capability array alive via explicit documented ownership, not incidental call-stack lifetimes.
- Verify drop order for attribute list vs referenced memory and AppContainer profile/SID; all Win32 handles freed on every success/error branch.
- Make a deterministic Rust-vs-C# parameter/lifetime comparison, scoped diagnostic on residual error 87, and native test proof.

### Explicitly out of scope

- Changing `ManagedProcessService` production `SandboxRequest::Required` behavior, implementing ACE leases, provisioning privileged identities, changing shell profiles, or continuing speculative cwd/flag variations without isolated evidence.

## 6. Required production changes

### Win32 ownership
Refactor `WindowsProcessAttributes` to store the stable `SECURITY_CAPABILITIES` and any `SID_AND_ATTRIBUTES` arrays and their owning SID/profile lease. Ensure attribute list destruction precedes freeing all referenced values. Use ownership/lifetimes or a documented invariant to prohibit storing a dangling borrowed SID. Explicit null capability pointer and count=0.

### Tests and diagnostics
Keep `WindowsSuspendedAppContainerProcess::create` in the test/experimental launch path. Preserve exact `CreateProcessW` diagnostic, one-time effective flag/env/argv and relevant SID facts without exposing secrets. Compare Rust `STARTUPINFOEXW::StartupInfo.cb`, `PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES` constant and value layout with the passing C# fixture.

### Build/guards
Add regression that inspects pointer backing lifetime and performs at least one native launch after deliberate stack churn; no self-reported success without querying `TokenAppContainerSid`, Job membership, and observing exit.

## 7. Ordered work packages

### WP-A — Lifetime and layout audit
Own the `SECURITY_CAPABILITIES` value for the attribute-list lifetime and audit each FFI pointer/array; verify documented Microsoft API contract. Evidence: code review with explicit ownership assertions; ABI layout/size tests where meaningful.

### WP-B — Native Rust fixture repair
Run existing `appcontainer_target_is_in_job_and_has_package_sid_before_resume` and `appcontainer_verification_failure_keeps_target_suspended`. Ensure queried SID equals created profile, Job assignment occurs before resume, failure path never writes a marker.

### WP-C — Comparator and bounded diagnosis
Run unchanged C# fixture in the same hosted workflow revision. If Rust still returns 87, add a single parameter-by-parameter comparison and isolate one variable at a time (e.g. attributes/flags/working-directory); publish actual observed errors without inventing a new backend.

### WP-D — Guard against regression
Test that the attribute struct allocation remains alive through CreateProcess (one direct regression test plus a source contract comment/static assertion, not brittle allocation-address guesses).

## 8. Failure, cancellation, restart, and contention semantics

On failure before resume, drop process only after terminate-and-wait; attribute-list and profile drop order must be safe on all unwinds/errors. Concurrent AppContainer tests use distinct package names and owned allocations; no static mutable shared SID. Test-only delayed launch should not leave dangling SID if setup scope ends. Cancellation is not production-integrated in C008.

## 9. Compatibility and migration

No storage, protocol, config or release change; no new full-host permission. Preserve Windows Rust 1.89 compatibility and installed Unix builds. No change to AppContainer profile name derivation or helper trust policy unless a verified bug requires it.

## 10. Required tests

- Win32 layout/lifetime unit tests with nonempty/empty capability arrays as applicable; negative `UpdateProcThreadAttribute` failure with resource cleanup.
- Native hosted test: AppContainer token SID, pre-resume Job ownership, successful child exit, forced verification failure, stack-lifetime regression.
- Existing disposable secret-read/outside-write fixture and Windows launcher suite must pass on **the same revision**; existing Unix quick/static guards stay green.
- If a failing native proof cannot run on the host, record blocked, not pass.

## 11. Required verification commands

On hosted Windows MSVC:

`cargo test --locked -p codegg --lib managed_process::windows::tests::appcontainer_ -- --nocapture`
`cargo test --locked -p codegg --test windows_process_launcher -- --nocapture --test-threads=1`
`./research/windows-native-sandbox/appcontainer-access-proof.ps1` (PowerShell shell)

On supported Unix:

`cargo fmt --all -- --check`
`python3 scripts/check_sandbox_contract.py`
`python3 scripts/check_execution_ownership.py`
`bash scripts/verify.sh quick`

The closure must record exact versions, run IDs, skipped coverage and failures. Do not mark tests passed if the hosted runner has no rights to execute them. New test filenames are prescribed as targets to create, not files assumed to exist today.

## 12. Documentation updates

`architecture/security.md` only if guarantee descriptions change; `research/windows-native-sandbox/appcontainer-access-proof.md` for actual new evidence; M004 corrective roadmap/registry and C008 closure.

## 13. Acceptance criteria

- Rust hosted AppContainer child starts, token SID matches expected profile, Job membership is verified before resume, and process terminates normally.
- The injected failure does not resume a target and cleans owned handles/profile.
- Attribute-list and referenced security capabilities lifetimes are explicitly safe through deletion.
- Windows constrained `SandboxRequest::Required` stays unavailable and never falsely reports filesystem enforcement.

## 14. Stop conditions

Stop if `CreateProcessW` still fails after proven pointer lifetime correction; do not repeatedly tweak unrelated flags/cwd without comparator evidence. Stop for ABI misunderstanding, FFI soundness hazards or broader change to authorization. Document measured deltas and replan within this corrective if needed.

## 15. Closure evidence required

Record the exact bug mechanism, pointer ownership before/after, Microsoft API lifetime reference, native hosted workflow URL/Windows build/test counts, comparison to unchanged fixture, error 87 result, forced failure cleanup, Unix guards, and C009 unblock assessment.

Do not edit the historical `plans/closure/windows-native-sandbox/004-status.md` to turn its failures into successes. Create `plans/closure/windows-native-sandbox/008-status.md` and make a later M004 requalification/closure decision from aggregated evidence.

## 16. Handoff notes

Official `lpValue` lifetime contract: https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute . C008 can run without a privileged VM. Never treat a passing token/Job experiment as production containment.
