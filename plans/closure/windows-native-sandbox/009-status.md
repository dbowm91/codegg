# Windows Native Sandbox Corrective C009 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/windows-native-sandbox/009-restricted-launch-managed-process-integration.md`

Source subsystem roadmap:

- `plans/subsystems/windows-native-sandbox-roadmap.md#m004-corrective-execution-sequence`

Repository baseline reviewed: `338d2364cda41d81400403b575f16e12998ad146`

Implementation commits:

- `75d7653c4` — supply AppContainer profile-local environment paths for restricted launches.
- `338d2364c` — isolate the AppContainer environment launch fixture and complete hosted qualification.

Hosted Windows workflow: <https://github.com/dbowm91/codegg/actions/runs/38196751443/job/114641001541>

## 1. Executive finding

C009 is closed as an infrastructure milestone. A test-only restricted AppContainer child now runs through the canonical `ManagedProcessService` lifecycle, including controlled stdio, filtered environment, exit accounting, deadline, cancellation, and filesystem-root rejection. The child token and Job membership are verified before resume, and launch/setup failures stay fail-closed.

This does not qualify filesystem ACL enforcement or enable production constrained requests. The integration is explicitly test-only, supplies no allowed filesystem roots, and reports no filesystem guarantee. C010 may proceed with runner-owned disposable-directory ACL leases. M004 remains blocked pending C010 and C011 evidence; no Windows filesystem `Enforced` or network-isolation claim is made.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Restricted child uses the canonical managed-process owner | `windows_restricted_managed_process` hosted integration suite | pass | Five tests passed through `ManagedProcessService`; no second execution service was introduced. |
| Token SID and Job ownership precede target execution | AppContainer launch ordering/cleanup suite | pass | Five tests passed, including pre-resume ownership and failure cleanup. |
| Stdio, exit, environment, timeout, and cancellation follow managed-process behavior | Hosted restricted integration suite | pass | Real pipes and exit status; timeout/cancellation cleanup exercised. |
| Handle inheritance is limited to intended standard handles | Launcher and restricted-process tests; failure-injection suite | pass | Restricted handle-list path passed. |
| Setup failures do not run the target | Failure-injection tests | pass | Four restricted failure-injection tests passed, including Job assignment and pipe setup failure. A narrower suspended-target filter selected zero tests because its name was stale; the broader filter exercised the applicable cases. |
| AppContainer environment is valid for hosted service execution | Managed environment block control test | pass | One native Windows test passed using profile-local `LOCALAPPDATA`, `TEMP`, and `TMP`. |
| Required Windows requests remain unavailable until ACL/security qualification | `prepare_launch_argv`, static sandbox contract guard, capability/status tests | pass | Production backend remains disabled; no filesystem enforcement status can be emitted by this fixture. |
| Unix and repository regressions remain green | `scripts/verify.sh quick` on the reviewed revision | pass | Quick guards and workspace all-target check passed. |

## 3. Production implementation evidence

`ManagedProcessService` now owns the restricted child lifecycle alongside its existing Windows launch path. The Windows child uses the repaired C008 attribute ownership, a narrow standard-handle list, explicit process/Job handles, and a bounded status path. Token SID and Job assignment are checked while suspended, before the single resume. Child termination and Job cleanup precede waiting for pipe completion.

The test-only policy seam is excluded from release binaries. Filesystem roots remain empty for this integration; the service rejects constrained production requests while the ACL lease and adversarial qualification gates are incomplete. The hosted runner exposed that AppContainer processes need profile-local environment directories: restricted launches now create and use the package-local `AC\\Temp` path for `LOCALAPPDATA`, `TEMP`, and `TMP`, while preserving relevant drive environment entries and the requested working-drive value.

## 4. Verification executed

### Hosted Windows evidence

Workflow `38196751443`, job `114641001541`, passed on revision `338d2364cda41d81400403b575f16e12998ad146`:

- OS: Microsoft Windows Server 2025 Datacenter, version `10.0.26100`, build `26100`.
- Rust: `1.89.0 (29483883e 2025-08-04)`, `x86_64-pc-windows-msvc`.
- AppContainer filesystem proof: passed.
- Windows managed process launcher: 8 passed.
- Restricted failure-injection coverage: 4 passed.
- Managed environment block/AppContainer control: 1 passed.
- Status-frame rejection: 1 passed.
- Restricted `ManagedProcessService` integration: 5 passed.
- AppContainer launch ordering and cleanup: 5 passed.
- Native Windows shell and policy tests: 13 passed.
- A narrower suspended-target filter ran 0 tests because the selected name no longer matched; the broader restricted failure-injection filter ran the relevant Job and pipe setup cases successfully.

### Cross-host verification

On the same revision:

```bash
cargo fmt --all -- --check
git diff --check
python3 scripts/check_sandbox_contract.py
python3 scripts/check_execution_ownership.py
bash scripts/verify.sh quick
```

All commands passed. Quick verification included repository guards and `cargo check --workspace --all-targets --locked`.

No privileged isolated Windows VM was available. C011-only tests requiring that environment were not run and are not represented as passed.

## 5. Invariant review

- One canonical process execution owner remains `ManagedProcessService`; the test seam does not create a standalone sandbox executor.
- Required constrained Windows requests fail before the target can run until all guarantees are qualified.
- Requested and obtained sandbox guarantees remain distinct. The fixture does not report filesystem enforcement or network isolation.
- AppContainer token SID and Job membership are verified before resume; stdio inheritance is restricted to the intended standard handles.
- Timeout, cancellation, and startup failure terminate the owned Job and account for descendants; the parent/daemon handles are not inherited.
- The profile-local environment fix avoids exposing the host user's writable profile paths to the restricted child.

## 6. Failure and recovery review

Job-assignment failure and pipe-setup failure were injected and passed without target execution. Status-frame rejection and managed-environment validation passed. The native suite exercised timeout and cancellation, with child output flushed before cancellation observation. The owned process handle remains authoritative if the child exits before waiter attachment, and Job teardown precedes waiting for pipe EOF.

Durable ACL mutation, crash recovery across daemon restart, path identity drift, concurrent ACL leases, and third-party descriptor conflict are not C009 scope; C010 owns these cases. Privileged credential/IPC and reparse/hardlink/ADS adversarial cases remain C011 scope.

## 7. Migration and compatibility review

No storage schema, public protocol, approval policy, or configuration migration was introduced. The new test support stays behind `windows-sandbox-test-support`. Unix launch paths and their policy were unchanged. Production Windows `Required` behavior remains fail-closed.

## 8. Security review

The target remains suspended until its AppContainer SID and Job ownership are checked. Handle inheritance is bounded to standard I/O. The status channel is bounded and rejects malformed frames. Environment construction filters inherited values and sets profile-local writable directories. No filesystem grant is applied by C009, no production backend is selected, and no network restriction is claimed.

## 9. Documentation and operations

Updated architecture ownership, security, and testing documentation with the test-only lifecycle boundary and its limitations. The roadmap and registry now record C009 closure and make C010 ready. Historical M004 blocked evidence at `plans/closure/windows-native-sandbox/004-status.md` is unchanged.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Filesystem read/write grants and durable restoration are not established by this process-control fixture. | Windows filesystem-constrained production execution remains unavailable. | C010 must prove runner-owned disposable-root lease behavior and recovery; C011/M004 remain gated on privileged VM adversarial evidence. |
| low | One narrowly filtered failure-injection command selected zero tests because its filter was stale. | No lost coverage: the broader restricted failure-injection command ran the relevant cases. | Keep closure evidence explicit; correct or remove redundant filter in future workflow maintenance. |

## 11. Roadmap disposition

C009 is closed and its hard dependency is satisfied. C010 is ready to begin using runner-owned disposable roots; it does not require a privileged VM for its bounded infrastructure work. C011 remains blocked on C010 and a privileged isolated Windows VM. M004 remains blocked, and production constrained Windows requests remain unavailable.

## 12. Registry updates

- `plans/registry.md`: mark C009 closed, add its closure/workflow evidence, and make C010 ready.
- `plans/subsystems/windows-native-sandbox-roadmap.md`: mark C009 closed and C010 ready; preserve C011 and M004 as blocked.
