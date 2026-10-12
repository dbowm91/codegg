# Windows Native Sandbox Corrective C014 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/windows-native-sandbox/014-projection-appcontainer-compatibility-and-deny-boundary.md`

Source subsystem roadmap:

- `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0019-projection-successor-milestones`

Repository baseline reviewed: `8ad76e640ba2e94308ad7361f65d33d5ccc5f213`

Implementation commits:

- `b2fffe07166075c336175594f97b04277a0fa541` — added the C014 Windows-only AppContainer projection and host-probe harness.
- `2242f8cb041d1821e21310a9f4410a799d31a8ca` — granted traversal for disposable test projection roots.
- `315bc69510410cde6094c85e4c75cfee9e2918f1` — placed the projection under the test AppContainer profile's private temp root.
- `0044d2f338d13ccee4a19f7f52efb2a5a93fc97a` — bounded the inherited-handle probe and added progress output.
- `f21437c54b2b58dd113902410d638e15379d56eb` — kept PowerShell startup cwd on the Windows runtime.
- `06de5a356b799029a37dde4d1b5228f726747a47` — used .NET APIs for the shell probe.
- `8ad76e640ba2e94308ad7361f65d33d5ccc5f213` — captured Git process-start output and diagnostics.

Native evidence: [Windows process-launcher workflow 38316065754](https://github.com/dbowm91/codegg/actions/runs/38316065754), exact SHA `8ad76e640ba2e94308ad7361f65d33d5ccc5f213`.

## 1. Executive finding

C014 closes **negative**. On a hosted Windows Server 2025 runner, the test-only AppContainer child successfully ran Windows PowerShell and read/wrote a newly created disposable projection. The required minimum Git smoke then failed during `git init` with `fatal: could not open '/dev/null' for reading and writing: Permission denied`. The child exited with status 1 after 2,213 ms. Because Git is a minimum C014 requirement, the tested AppContainer grant envelope does not provide a viable minimum tool subset. The experiment stopped before direct/aliased host reads, IPC, inherited-handle, Cargo, or Python probes; this closure makes no host-denial or broader toolchain claim.

This is the negative feasibility disposition required by the plan, not a production capability. C015–C018 remain blocked. Proposed ADR-0020 asks maintainers to compare a dedicated identity, a separate OS-managed isolation boundary, and retaining constrained Windows execution as unavailable. Windows production `SandboxRequest::Required` remains unavailable; filesystem and network enforcement remain unclaimed.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Launch a verified AppContainer child through the existing test-only managed-process boundary | Workflow `38316065754`, C014 test selected count 1; result reported `TestOnlyAppContainer` only after child return | pass for child identity/Job setup | Child started and exited; production enforcement status was not returned. |
| Read and write a newly created projection file | Child stdout `probe=started`, `projection=read-write-ok` | pass | Projection resolved beneath the disposable AppContainer profile-local temp root. |
| Run the minimum shell and Git subset without broad user-profile grants | Same C014 child; Git init stderr `fatal: could not open '/dev/null' for reading and writing: Permission denied` | fail | No runner-user profile ACL was changed. Git initialization could not proceed within the tested envelope. |
| Determine Cargo/Python compatibility | Not reached | not run | Git failed before those invocations. |
| Prove denied direct, junction, hardlink, ADS, sibling, workspace, pipe and inherited-handle access | Probe code exists, but execution stopped at Git initialization | not run | No denial result is claimed. |
| Measure warm/cold p50/p95 startup and staging overhead | One 2,213 ms child elapsed time | incomplete | One failed run is not a latency distribution and no staging materializer exists. |
| Preserve fail-closed production behavior | Code and guards; no production backend was added or registered | pass | Windows constrained requests remain unavailable; no filesystem `Enforced` status is claimed. |
| Preserve existing-object forensic regression | Workflow C013 prerequisite `c013_forensic_existing_object_acl_and_alias_probe`, selected count 1 | pass | Its existing negative finding remains unchanged; this is not C014 host-denial evidence. |

## 3. Production implementation evidence

No production sandbox, materializer, snapshot, promotion, protocol, or storage behavior was added. The C014 executable test remains behind `windows-sandbox-test-support`. Its test-only launcher creates a disposable AppContainer profile temp directory, grants that disposable root, and starts PowerShell with a sanitized environment. C014-specific changes do not grant access to the runner user's profile or modify original workspace ACLs.

The harness stages a synthetic credential, hardlink, ADS, junction, sibling file, synthetic named pipe, and inheritable synthetic file handle. These are fixture setup only: because Git initialization failed before the remaining PowerShell script statements, none of the intended denial probes executed. No evidence from fixture creation is presented as evidence of denial.

## 4. Verification executed

### Commands run

Hosted Windows Server 2025 x86_64 MSVC, Rust 1.89.0, exact SHA `8ad76e640ba2e94308ad7361f65d33d5ccc5f213`:

```powershell
cargo test --locked -p codegg --features windows-sandbox-test-support --lib c013_forensic_existing_object_acl_and_alias_probe -- --nocapture --test-threads=1
cargo test --locked -p codegg --features windows-sandbox-test-support --test windows_projection_feasibility -- --nocapture --test-threads=1
```

Cross-host verification on the implementation worktree:

```bash
cargo fmt --all -- --check
python3 scripts/check_sandbox_contract.py
python3 scripts/check_execution_ownership.py
git diff --check
cargo check --locked -p codegg --features windows-sandbox-test-support --test windows_projection_feasibility --target x86_64-pc-windows-gnu
bash scripts/verify.sh quick
```

### Results

- Runner metadata: Microsoft Windows Server 2025 Datacenter, version `10.0.26100`, build `26100`; `rustc 1.89.0 (29483883e 2025-08-04)`, host `x86_64-pc-windows-msvc`. Hosted runner privilege evidence reported administrator role `True`, high-integrity token `True`, and UAC enabled `True`.
- C013 prerequisite: 1 selected test passed (`1 passed`, `5505 filtered out`).
- C014: 1 selected test failed (`0 passed, 1 failed`, `5505 filtered out`). The child exited normally with status 1 after 2,213 ms. Captured stdout was `probe=started`, `projection=read-write-ok`, `git=init-start`, and Git's `/dev/null` permission error.
- All later Windows workflow steps were skipped after the C014 failure. The workflow therefore concluded failure; it did not run the remaining launcher/regression steps on this SHA.
- `cargo fmt`, both sandbox/execution guards, `git diff --check`, the Windows GNU target compile check, and `scripts/verify.sh quick` passed locally. These checks do not substitute for the missing C014 denial matrix.
- Earlier exact-SHA Windows attempts `38306543785`, `38308251741`, `38310063232`, `38311694822`, and `38313474053` exposed harness issues (runner temp ACL, startup path, unbounded handle read, and unavailable PowerShell utility cmdlets). The final run fixed those issues and reached the actual minimum Git compatibility failure. The final result is not a compile-only or preflight refusal.

## 5. Invariant review

- **No production Windows constrained admission:** preserved; no backend registration, `Enforced` result, or FullHost fallback was introduced.
- **No mutation of real user ACLs:** preserved; only the disposable AppContainer package temp root received the test-only grant. No runner-user profile ACL, workspace ACL, named-pipe DACL, original hardlink, or user secret was modified.
- **AppContainer and Job facts are not overclaimed:** child startup and test-only token/Job verification passed, but no filesystem or network enforcement guarantee is claimed.
- **Host-denial assertions are not fabricated:** all direct, alias, IPC and handle probes were after the failed Git step and are recorded as not run.
- **Unix and existing FullHost behavior:** unchanged.

## 6. Failure and recovery review

The Git process was launched inside the AppContainer, then exited unsuccessfully while opening `/dev/null`; the parent captured stderr and returned failure. The C014 harness fails the selected test on this result, and the workflow skipped subsequent steps. There was no timeout, cancellation, child/grandchild exercise, promotion, or restart path in this C014 run. The disposable projection and profile are test-run resources; no durable staging state was introduced. Recovery or production use of the failed projection is not supported.

## 7. Migration and compatibility review

No storage schema, public API, protocol, configuration, or user workspace migration changed. The experiment proves the profile-local disposable projection can be accessed by its AppContainer child, but the minimum Git workflow is incompatible with the tested grants. Existing Windows constrained execution remains unavailable. C015–C018 cannot inherit a compatibility assumption from this test.

## 8. Security review

This run does not qualify original-host access denial. The child never reached the direct path, junction, hardlink, ADS, sibling, named-pipe, or inherited-handle probes. Common OS access, registry access, network access, descendant escape resistance, and sensitive-path behavior remain unmeasured by C014. No network guarantee is made.

The observed Git error is treated as runtime incompatibility under the tested AppContainer policy, not proof of a Windows-wide limitation. Any renewed feasibility work must use a separately accepted architecture and independently measure its effective grants and host-denial boundary. C011/M004 remain blocked.

## 9. Documentation and operations

- Updated the C014 plan to its implemented negative-feasibility disposition.
- Added this closure record with exact-SHA hosted evidence and explicit unrun probes.
- Updated the Windows native sandbox roadmap and registry; no dependent implementation plan was made ready.
- Added proposed ADR-0020 for maintainer review; it authorizes no account, ACL, firewall, or VM provisioning.
- `verify.sh quick`, formatting, sandbox contract, execution ownership, and Windows GNU compile guards passed locally.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | The tested AppContainer policy cannot run the minimum Git `init` smoke because access to `/dev/null` is denied. | The bounded projection/AppContainer line has no qualifying minimum tool subset; C015–C018 cannot proceed. | Review proposed ADR-0020 before creating a successor feasibility plan. Keep production Windows constrained execution unavailable. |
| high | C014 direct/alias/IPC/inherited-handle denial matrix did not execute. | No host-read denial, IPC isolation, inherited-handle, or network guarantee is established. | Any future design must rerun a complete native denial matrix under its own accepted architecture. |

## 11. Roadmap disposition

C014 is closed negative. ADR-0019's experiment stops at its explicit compatibility gate. C015–C018 remain blocked and no materializer, staged launcher, promotion path, or production Windows sandbox is authorized. Proposed ADR-0020 compares dedicated identity, another OS-managed boundary, and retaining constrained Windows unavailability; it is not accepted. C011/M004–M006 remain blocked, and M007 remains deferred.

## 12. Registry updates

- Moved C014 from active/dependency-ready to recently closed as negative feasibility.
- Left C015–C018 blocked; this closure unblocks no implementation plan. A future plan requires an accepted successor architecture decision and a new strict-positive feasibility gate.
- Updated the Windows native sandbox roadmap and the current execution-order gate with the exact Git compatibility result.
- Added proposed ADR-0020. ADR-0019 remains unchanged as an accepted historical experiment-direction record.
