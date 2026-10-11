# Windows Native Sandbox Corrective C008 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/windows-native-sandbox/008-appcontainer-ffi-attribute-lifetime-corrective.md`

Source subsystem roadmap:

- `plans/subsystems/windows-native-sandbox-roadmap.md#m004-corrective-execution-sequence`

Historical M004 blocked evidence remains unchanged at
`plans/closure/windows-native-sandbox/004-status.md`.

Implementation revision: `280a73424f3d8cd36a8495a3ca85dd7704a07d27`

Hosted Windows workflow: <https://github.com/dbowm91/codegg/actions/runs/38178437505>

## 1. Executive finding

C008 is closed. `WindowsProcessAttributes` now owns a boxed
`SECURITY_CAPABILITIES` value and borrows the profile that owns its AppContainer
SID for the attribute-list lifetime. Its destructor deletes the Win32
attribute list before Rust releases those owners. This removes the reviewed
stack-local `lpValue` lifetime violation and prevents the profile SID from
being freed while the list can still refer to it.

The repaired Rust launch passed on Windows Server 2025. The test queried the
child token's AppContainer SID, verified the requested profile SID, verified
Job membership before resume, and observed a normal exit after stack churn.
The injected verification failure also kept the target suspended and cleaned
up without running it. This closes only the C008 launch-proof corrective; it
does not qualify filesystem ACLs, activate the Windows sandbox backend, or
close historical M004.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Attribute value and profile SID remain valid through attribute-list deletion | `WindowsProcessAttributes<'profile>` owns `Box<SECURITY_CAPABILITIES>` and borrows `WindowsAppContainerProfile`; `Drop` deletes the list first | pass | `SECURITY_CAPABILITIES` uses null capabilities, count zero, and reserved zero. |
| Rust ABI fields match the C# fixture contract | `appcontainer_attribute_owns_abi_value_and_profile_sid` | pass | Test checks attribute constant `0x00020009`, `SECURITY_CAPABILITIES` and `STARTUPINFOEXW` sizes for pointer width, package SID, and empty capability fields. |
| Stack-local regression is exercised by a real native launch | `appcontainer_target_is_in_job_and_has_package_sid_before_resume` | pass | Test churns 32 KiB of stack after setting the attribute and before `CreateProcessW`. |
| Child token has the requested package SID | `appcontainer_target_is_in_job_and_has_package_sid_before_resume`; workflow `38178437505` | pass | `GetTokenInformation(TokenAppContainerSid)` and `EqualSid` checked before resume. |
| Job ownership precedes first target instruction | `appcontainer_target_is_in_job_and_has_package_sid_before_resume`; workflow `38178437505` | pass | `IsProcessInJob` runs in the assignment/resume callback. |
| Verification failure does not resume the target | `appcontainer_verification_failure_keeps_target_suspended`; workflow `38178437505` | pass | Target remained in `WAIT_TIMEOUT` while suspended and was terminated during cleanup. |
| C# comparator and native Rust suite run on one revision | Workflow `38178437505` | pass | C# filesystem proof precedes Rust launcher and AppContainer tests in the same job. |
| Unix verification and ownership guards remain green | `bash scripts/verify.sh quick`; sandbox and execution-ownership guards | pass | Exact command results are recorded below. |

## 3. Hosted Windows evidence

Workflow `38178437505` passed on implementation revision
`280a73424f3d8cd36a8495a3ca85dd7704a07d27`:

- OS: Microsoft Windows Server 2025 Datacenter, version `10.0.26100`, build
  `26100`.
- Rust: `rustc 1.89.0 (29483883e 2025-08-04)`, target
  `x86_64-pc-windows-msvc`.
- The unchanged C# AppContainer filesystem access proof passed.
- `cargo test --locked -p codegg --test windows_process_launcher -- --nocapture --test-threads=1`:
  8 passed, 0 failed.
- `cargo test --locked -p codegg --lib managed_process::windows::tests::failed_job_assignment_never_runs_the_suspended_target -- --nocapture`:
  1 passed, 0 failed.
- `cargo test --locked -p codegg --lib managed_process::windows::tests::appcontainer_ -- --nocapture`:
  3 passed, 0 failed. This includes the ABI/value ownership test, stack-churn
  token/Job launch proof, and suspended verification-failure cleanup proof.
- Selected Windows native shell and policy tests passed.

The first attempt, workflow `38177710427`, stopped during test compilation
because the new test module had not imported
`PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES`. The import was added in
`280a734`; this was a test-source compile error, not a native launch failure.
The complete workflow then passed on the corrected revision. Earlier Rust
`CreateProcessW` error-87 runs remain historical failed evidence for the
pre-correction code.

## 4. Rust/C# parameter comparison

Both fixtures use the created package SID, null capability-array pointer,
zero capability count, reserved zero, one security-capabilities process
attribute (`0x00020009`), extended startup info, and suspended/no-window
creation flags. C# uses `Marshal.SizeOf`; Rust uses `size_of`, with the ABI
sizes and constant checked by the new native unit test. The hosted workflow
runs the unchanged C# fixture and Rust tests on the same Windows revision.

The prior Rust code passed a pointer to a stack-local capabilities struct and
returned before `CreateProcessW`. The C# fixture allocates its value with
`Marshal.AllocHGlobal` and holds it through `CreateProcess`, then releases it
before deleting the attribute list. This makes it a parameter/result
comparator, not proof of the complete lifetime contract. Microsoft's
`UpdateProcThreadAttribute` documentation requires `lpValue` to remain valid
until `DeleteProcThreadAttributeList` destroys the list:
[UpdateProcThreadAttribute](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute).
The Rust owner now enforces that full lifetime, including the profile SID.

## 5. Unix and repository verification

On the implementation tree at revision `280a734`:

- `cargo fmt --all -- --check` passed.
- `python3 scripts/check_sandbox_contract.py` passed.
- `python3 scripts/check_execution_ownership.py` passed.
- `python3 scripts/check_work_plan_repository_binding.py` passed after its
  storage-version assertion was corrected to compare against the highest
  dispatched migration (the repository is at v70, including required v68).
- `bash scripts/verify.sh quick` passed, including all quick guards and
  `cargo check --workspace --all-targets --locked`.

The local host is Linux, so Windows native behavior is evidenced only by the
hosted Windows workflow above. No Linux or macOS claim is inferred from the
Windows run.

## 6. Security and compatibility disposition

- AppContainer creation remains a test/experimental launch path; no production
  Windows sandbox backend or `SandboxRequest::Required` path was activated.
- No filesystem `Enforced` or network-isolation claim is made. Network remains
  unrestricted unless separately qualified by M005.
- The profile borrow prevents safe Rust from dropping/moving the profile while
  `WindowsProcessAttributes` can still refer to its SID.
- Attribute-list destruction occurs explicitly before the boxed value is
  released. The null capability pointer and zero count are deliberate.
- Unix Landlock/Seatbelt code, storage, protocol, approval, and process
  ownership contracts are unchanged.

No unresolved high or medium finding remains in C008 scope. Production Windows
filesystem enforcement, ACL leases/recovery, privileged IPC, hardlink/ADS and
reparse-point adversarial cases remain out of scope and unqualified.

## 7. Roadmap and dependency disposition

C008 is strictly closed. C009 is unblocked and ready to integrate the repaired
restricted child through `ManagedProcessService` using a test-only prepared
policy path. C009 must preserve fail-closed production admission. C010 remains
blocked on C009 and owns journaled ACL leases and recovery. C011 remains blocked
on C010 and privileged disposable Windows VM evidence. M004 remains blocked;
its historical closure is not edited, and no constrained Windows execution is
reported as enforced.
