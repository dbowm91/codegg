# Windows Native Sandbox C035 — Closure Status

Status: **conditionally closed (bounded negative candidate; no production admission)**

Source implementation plan:

- `plans/implementation/windows-native-sandbox/035-mxc-native-helper-and-handle-boundary-corrective.md`

Source subsystem roadmap:

- `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0023-mxc-helper-diagnostic-corrective`

Repository baseline reviewed: `80139df351e9a77f46f7b09f904a0d6e854939b4`

Implementation commits:

- `1a26544e` — add isolated C035 native helper, harness, and opt-in ARM64 workflow
- `f55e360a` — provide the stage-local Windows profile variables required by MXC
- `4a7ec980` — retain ARM64 PE import and compiler provenance
- `8138676f` — capture structured exceptions from invalid handle probes
- `3fbe6201` — add an explicit host-path read and preserve per-request denial evidence

Native evidence: [Windows MXC C035 run 38417084880](https://github.com/dbowm91/codegg/actions/runs/38417084880), exact SHA `3fbe620134ee3fff31d5c6f195a8573ab6941b2f`. The workflow job succeeded as evidence collection; its versioned report says `qualified=false`.

## 1. Executive finding

C035 isolated and improved the C033 startup diagnosis. A separately compiled ARM64 helper reached its first user entry both in an uncontained `CreateProcessW` control and in the MXC BaseContainer child. The uncontained control read the Everyone-readable synthetic file through both its path and a verified inherited handle. The MXC child wrote a fresh nonce/PID-bound stage proof, reported its token user/groups/integrity/type, and received `ERROR_ACCESS_DENIED` when opening the same synthetic file by path.

The inherited-handle gate did not qualify: the MXC child reported the supplied handle absent and its probe raised `STATUS_INVALID_HANDLE` (`0xC0000008`). C035 also did not produce its separately named MXC denial JSON despite the path denial. These results do not establish inherited-handle denial or independently attributable C035 telemetry. The native candidate remains negative and no production capability is admitted. Further evidence requires a new explicit architecture decision because ADR-0025 authorized only this corrective.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| ARM64 helper provenance and early entry | Run `38417084880`, `C035_HELPER`; PE machine `AA64`/ARM64, helper SHA-256 `ac843e7069fd80e8d0fe6fe61c0d3ce76fe22bf55b7cedcdce2bac9d2cfe4af2`, MSVC 19.51.36260.0, imports include `ADVAPI32.dll` and `KERNEL32.dll` | pass | C helper was compiled separately from the MXC Rust harness. |
| No-container startup and real inherited-handle positive | `C035_REPORT.control_status`, `control_entry`, and `control_proof` | pass | `CreateProcessW` used `bInheritHandles=TRUE` and an explicit handle list. Child PID 8264 reached entry and read exactly the synthetic bytes by path and handle. `GetHandleInformation` reported an inheritable disk handle. |
| MXC native child reaches user entry | `C035_REPORT.mxc_entry`, PID 9096; same run nonce as the report | pass | The minimal helper entered under selected ProcessContainer/BaseContainer and returned `Exited(0)`. This does not isolate why the larger C033 Rust harness previously exited `0xC0000142`. |
| MXC child cannot read synthetic host file by path | `C035_REPORT.mxc_proof.host_path_read_error=5`, `host_path_read_ok=false`, `host_path_secret_match=false` | pass for this path operation | The synthetic file SHA-256 stayed `1f618c08f568e54209e8442db5b74f0cbb6a34ae7e7147d8a2e502737051e4fc`; the synthetic ACL was stable from MXC launch through return. No original host object or ACL was touched. |
| MXC inherited-handle boundary | `C035_REPORT.mxc_proof.handle_present=false`, `probe_exception=3221225480` (`0xC0000008`) | fail / unqualified | The uncontained positive is valid, but the running MXC child's handle probe raised `STATUS_INVALID_HANDLE`. This is not counted as a handle denial. |
| Fresh stage write and child identity | `C035_REPORT.mxc_proof`; child PID/nonce match entry; marker `child-proof`; token user SID, 14 group SIDs, integrity SID `S-1-16-4096`, token type 1 | pass | The child created the nonce-specific proof file using `CREATE_NEW`; no marker was pre-created. |
| C033 baseline observations | Same-run C033 baseline artifact: normal read allowed; direct, junction, case, hardlink, ADS, extended-path, sibling reads, outside write, and pipe denied; Git, Python, and offline Cargo smoke succeeded; 21 non-truncated block-mode events | pass for tested observations | C033's separate copied Rust helper still exited `0xC0000142`. C033 synthetic ACLs and content were unchanged; its descendant was gone at return. |
| C035 per-request denial output | `C035_REPORT.mxc_denial_capture_present=false` and `mxc_denial_document=null`, despite the path result above | fail | The C035 request used its own nonce-specific output path and block mode, but no JSON document was found. This corrective does not claim C035 event attribution. |
| Early-exit negative classification | `C035_REPORT.pre_entry_nonzero_is_not_denial=true`; isolated unit test rejects absent, wrong-nonce, and mismatched-PID entry/proof evidence | pass | Nonzero-before-entry is not translated into a denial result. |
| Full strict-positive qualification | `C035_REPORT.qualified=false`; named failed gates B and D | fail / no admission | MXC inherited-handle result and C035 denial telemetry are incomplete. |

## 3. Production implementation evidence

No production implementation changed. The new C helper, Cargo harness, and opt-in workflow are isolated under `research/windows-native-sandbox/mxc-c035-harness/` and `.github/workflows/windows-mxc-c035-helper-corrective.yml`. C033/C034 historical closure records were not edited. MXC remains research-only; Windows constrained execution remains unavailable.

## 4. Verification executed

### Commands run

```bash
cargo fmt --manifest-path research/windows-native-sandbox/mxc-c035-harness/Cargo.toml -- --check
cargo check --target aarch64-pc-windows-msvc --locked --manifest-path research/windows-native-sandbox/mxc-c035-harness/Cargo.toml
cargo test --locked --manifest-path research/windows-native-sandbox/mxc-c035-harness/Cargo.toml
python3 scripts/check_sandbox_contract.py
python3 scripts/check_execution_ownership.py
bash scripts/verify.sh quick
git diff --check
```

### Results

- Isolated Rust formatting, ARM64 Windows cross-target check, and the focused evidence-validation unit test passed locally.
- Sandbox contract and execution-ownership guards passed.
- `scripts/verify.sh quick` passed locally on the implementation head.
- Hosted Windows C035 run `38417084880` completed all steps on the exact implementation SHA and uploaded the raw bounded evidence artifact. Workflow success is evidence collection success only; native qualification failed as recorded above.
- Hosted branch CI run `38417097457` was still in progress when this record was prepared. It is separate from the opt-in Windows evidence workflow and must not be represented as passing. See the final branch CI result linked from this run after completion.

## 5. Invariant review

- No backend registration, `ManagedProcessService` change, public API/configuration change, Windows `Required` enablement, or filesystem/network `Enforced` claim was added.
- All fixture files and profile directories were created under the disposable run root. Only the synthetic secret received the explicit broad-read ACEs; its contents hash and post-grant ACL snapshot remained stable during the MXC invocation.
- Capture mode remained `block`; the harness does not interpret missing output, startup failure, invalid handles, or nonzero exits as a denial.
- C033 and C034 historical records remain unchanged. The rejected ADR-0023 broker/guest choices and C034 no-admission decision remain effective.

## 6. Failure and recovery review

The uncontained child was bounded to 15 seconds and exited 0. The MXC child completed with `Exited(0)` after recording its stage proof and observations. The invalid inherited handle exception was caught and reported rather than terminating the child. C033's descendant process was absent at return in the same hosted run's baseline stage.

C035 did not independently exercise an MXC descendant tree, forced timeout cleanup, Job-owned orphan enumeration, crash/reboot cleanup, or concurrency. The SDK evidence does not expose those guarantees here. No such cleanup or recovery claim is made.

## 7. Migration and compatibility review

No product schema, protocol, dependency, installer, account, or migration changed. The Cargo harness pins MXC SDK 1.0.0 and runs under isolated Rust 1.93.1 without changing CodeGG's Rust 1.89 MSRV. No compatibility or rollback action is required for production.

## 8. Security review

The path read denial is limited to the synthetic C035 file and tested MXC BaseContainer on Windows 11 Enterprise 25H2 ARM64 build 26200 UBR 9457. The no-container child proved the fixture was readable. The MXC child reached entry and reported `ERROR_ACCESS_DENIED` for that path. This does not prove inherited-handle denial: the handle observation ended in `STATUS_INVALID_HANDLE`, so the result is unavailable for the required comparison. The absence of C035 denial JSON also prevents strict telemetry closure.

No secret outside the synthetic fixture was used. No user profile ACL, source object, account, service, firewall, or production policy was changed. No network behavior was tested. Windows `SandboxRequest::Required` remains unavailable; do not infer filesystem or network enforcement from this result.

## 9. Documentation and operations

The C035 workflow records OS edition/version/build/UBR/architecture, workflow run ID, helper PE machine/hash/imports/compiler, C033 baseline, and the versioned C035 report. It uploads a seven-day artifact. Its step summary explicitly reports native qualification as failed/incomplete independently of job success. Exact evidence is in run `38417084880`.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | The MXC child did not provide a usable inherited-handle operation result; `STATUS_INVALID_HANDLE` occurred after entry. | The candidate's handle boundary remains unknown. No product path uses this candidate. | Any further native handle investigation requires a new accepted architecture decision and a child observation that distinguishes a stripped handle from an access denial. |
| medium | C035 block-mode denial JSON was absent even though the MXC child observed path `ERROR_ACCESS_DENIED`. | Per-invocation event attribution is unverified for C035. | A future approved evidence pass must produce and validate a unique capture document or classify why the SDK cannot expose it. |
| low | C035 did not independently verify MXC Job-owned descendant cleanup after timeout/failure. | No C035 timeout/orphan-cleanup claim. | Require a separately bounded cleanup test before considering any successor candidate. |

## 11. Roadmap disposition

C035 is **conditionally closed as a bounded negative test-only candidate**. The startup, stage-write, and token-identity gaps were answered by the minimal helper. The inherited-handle and per-invocation telemetry gates remain incomplete. ADR-0025's authority expires here; these findings do not authorize another corrective run or production integration. Windows constrained execution stays unavailable, and C011/M004–M006/C015–C018 remain blocked.

## 12. Registry updates

- Remove C035 from active/dependency-ready work and record this conditional closure in `plans/registry.md`.
- Update the Windows native sandbox roadmap C035 status and link this closure.
- No blocked plan was unblocked: C035's result satisfies no C011/M004–M006/C015–C018 prerequisite. C030/C031 remain terminally blocked. Any future MXC work requires a new accepted ADR and implementation plan.
