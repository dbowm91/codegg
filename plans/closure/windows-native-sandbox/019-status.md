# Windows Native Sandbox Milestone 019 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/windows-native-sandbox/019-shared-native-security-probe-contract.md`

Source subsystem roadmap:

- `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0020-independent-successor-feasibility-2026-10-12`

Repository baseline reviewed: `2a39eb6eba4234ff865e8a92282797203dab681f` (`work/windows-native-sandbox-foundation`)

Implementation commits or pull requests:

- `33a708c9` — add the shared Windows actual-child probe contract and hosted workflow.
- `c26f36bf137bceccbceabf971d161dd34c8a60cb` — stage through the runner-owned AppContainer projection and preserve per-probe results.
- `2a39eb6eba4234ff865e8a92282797203dab681f` — run the unrestricted null-device comparison from a valid OS working directory.
- Hosted Windows run `38330137971`, job `115077792062`, passed on the exact code SHA `2a39eb6eba4234ff865e8a92282797203dab681f`.

## 1. Executive finding

C019's reusable actual-child oracle is complete and accepted. Its Windows Server 2025 run exercised a granted package-private stage and independently recorded the adversarial filesystem and IPC probes. The host credential, host and sibling DACLs, and source file identities remained unchanged. The C014 `/dev/null` cause remains unconfirmed: both the AppContainer child and unrestricted baseline reported a .NET `MethodInvocationException` when the PowerShell probe opened `\\.\NUL`; that result does not identify the underlying Win32 access or Git/MSYS behavior. The exact outcome is retained for follow-up and does not block C020/C021 under the C019 acceptance rule.

This is test-only infrastructure. It does not qualify or enable production Windows `SandboxRequest::Required`.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| A real allowed stage operation in a managed child | Run `38330137971`, `C019_PROBE` `projection_read_write` | pass | Child wrote and reread `stage-ok` in the per-child AppContainer-private temp projection; launcher verified the AppContainer SID and Job. |
| Direct and alias host-secret reads denied | Run `38330137971` per-action records | pass | Direct, junction, case alias, extended path, hardlink, ADS, and sibling probes each returned `denied` in a running child. The host file identity was volume `742408122`, file index `2533274790572592`, link count `2`. |
| Outside create/write and named pipe denied | Run `38330137971` per-action records | pass | Both actions returned `denied` in a running child. |
| Git, Python, Cargo compatibility isolated from security probes | Run `38330137971` per-action records | partial | Host had Git 2.55.0.windows.5, Python 3.12.10, and Cargo 1.89.0; their AppContainer child actions returned `failed`. The separate denial probes still ran. Their exact tool failure cause was not instrumented in this oracle. |
| C014 null-device hypothesis compared with unrestricted child | Run `38330137971`, both `null_device_access` records | partial | Both returned `denied:MethodInvocationException` from the .NET file-open call. No lower-level Win32 error or Git `/dev/null` syscall evidence was captured; cause remains unconfirmed. |
| UNC host-read fixture | Run `38330137971`, `unc_host_read_unavailable` | not run | No isolated SMB fixture exists on this hosted runner. |
| Hostile pre-opened/inherited handle transfer | Run `38330137971`, `inherited_handle_not_transferred` | not run | The managed launcher passes only its stdio handles; this experiment did not construct a hostile inherited handle. |
| Durable restart/replay behavior | Run `38330137971`, `restart_recovery_unavailable` | not run | The test-only process fixture has no durable request or startup recovery path. |
| Timeout, cancellation, descendant, and Job lifecycle coverage | Run `38330137971`, `windows_restricted_managed_process` | pass | 6 tests passed; no failures, ignored tests, or filtered tests. |
| Host fixture integrity | Run `38330137971` `C019_REPORT` | pass | Host and sibling DACL snapshots were unchanged; both before/after checks were true. Host credential contents were unchanged. DACL SHA-256 for each fixture root was `ef5786304e87f816a713a99fae3a6c637228a350642d0c820b6c648562fb5c5c`. |

## 3. Production implementation evidence

Added `.github/workflows/windows-native-probe-contract.yml` and `tests/windows_native_probe_contract.rs`. The harness launches each selected action through `ManagedProcessService` as a fresh child and emits a JSON observation containing backend, host, privilege, action, expected/observed state, file identity, child output, Job verification, timing, and provenance. The allowed stage uses the existing test-only launcher projection, which creates a per-child private AppContainer temp root. No production backend, public policy, admission behavior, dependency, or ACL lease was added.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all
cargo check --locked -p codegg --features windows-sandbox-test-support --test windows_native_probe_contract --target x86_64-pc-windows-gnu
bash scripts/verify.sh quick
git diff --check
cargo test --locked -p codegg --features windows-sandbox-test-support --test windows_native_probe_contract -- --nocapture --test-threads=1
cargo test --locked -p codegg --features windows-sandbox-test-support --test windows_restricted_managed_process -- --nocapture --test-threads=1
```

### Results

- Local Windows-target cross-check, `verify.sh quick`, formatting, and diff checks passed. The cross-check emitted existing non-fatal non-Windows-target warnings.
- Hosted run `38330137971` passed both selected suites: 1 C019 matrix test and 6 managed-process lifecycle tests.
- Host: Microsoft Windows Server 2025 Datacenter, version `10.0.26100`, build `26100`, DisplayVersion `24H2`, UBR `33438`; latest listed hotfix `KB5126052` (installed 2026-09-05).
- Runner identity: `runnervmfi6oq\runneradmin`, administrator and high-integrity. Toolchain: Rust/Cargo 1.89.0, Git 2.55.0.windows.5, Python 3.12.10.
- `git init/status`, Python, and Cargo child smoke tests were independently observed as failed. They are compatibility evidence, not denied-host results. The full-host null baseline launched successfully after the working-directory correction but produced the same managed `MethodInvocationException` as the restricted child.

## 5. Invariant review

- Production `SandboxRequest::Required` remains unavailable; the observed backend is explicitly `test_only_app_container`.
- No production backend registration or `Enforced` outcome was introduced.
- No original host/source ACL was leased or changed. Host and sibling DACL snapshots and credential contents matched before/after.
- No network isolation claim is made.
- Every negative path result cited above came from a running managed child with AppContainer SID and Job verification; unsupported paths are individually recorded as not run.

## 6. Failure and recovery review

Each matrix operation is isolated in a fresh process. The test workflow also ran the existing timeout, cancellation, descendant, and Job lifecycle suite (6 passing tests). The C019 fixture itself has no durable request state, so restart/replay is explicitly not run. C014 device diagnosis remains unconfirmed; the output captures the managed exception class and the unrestricted comparison but not the underlying kernel error.

## 7. Migration and compatibility review

No storage, protocol, configuration, or migration changes were made. The C019 executable remains gated by `windows-sandbox-test-support`; the workflow is a test-only Windows hosted-runner qualification path. Existing Windows AppContainer production availability is unchanged.

## 8. Security review

Only synthetic secrets were used. The runner-owned host and sibling fixtures were not given AppContainer allow ACEs. Direct/reparse/alias reads and an outside write were attempted by running children and denied. The host DACL hashes and secret contents were unchanged. No plaintext credential, persistent account, global device descriptor, or original workspace mutation was involved.

## 9. Documentation and operations

- The workflow records edition/build/UBR, installed hotfixes, privilege, and toolchain before running probes.
- Machine-readable `C019_REPORT` and individual `C019_PROBE` lines are emitted to the Actions log.
- `research/windows-native-sandbox/adr-0020-harness-comparison.md` now records the exact Server 2025 oracle results and the still-unconfirmed C014 device cause.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | C014 Git `/dev/null` failure is not root-caused; the .NET `\\.\NUL` probe reports `MethodInvocationException` in both restricted and unrestricted children. | Do not infer a device ACL defect or prescribe a host descriptor workaround. | Preserve this as an unconfirmed diagnosis; any later investigation must capture the Win32 result and Git/MSYS behavior on an isolated host. |
| medium | Git/Python/Cargo smoke commands failed in the AppContainer children; per-tool stderr/exit detail is insufficient to classify the failure. | The AppContainer fixture is not a viable developer-tool compatibility qualification. | C020/C021 run their own candidate-specific tool probes; do not reuse this as a candidate result. |
| low | UNC, hostile inherited handle, and durable restart probes were not run. | Those dimensions are absent from this fixture run. | Keep each listed as untested; do not claim their containment. |

## 11. Roadmap disposition

C019 is closed: its shared oracle acceptance criteria passed, and its remaining unsupported probes and C014 diagnostic uncertainty are explicit. C020 and C021 may proceed independently. This closure does not reopen C015–C018 or qualify any production Windows sandbox capability.

## 12. Registry updates

- Mark C019 closed in `plans/registry.md` and the Windows native sandbox roadmap.
- Mark C020 and C021 ready for their independent bounded candidate experiments; retain C022 blocked on their written dispositions.
- Record the C014 null-device cause as unconfirmed in the ADR-0020 comparison research; it is not a hard dependency for C020/C021 after this accepted oracle.
