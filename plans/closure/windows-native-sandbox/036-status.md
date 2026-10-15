# Windows Native Sandbox C036 — Closure Status

Status: **closed — bounded negative, no production admission**

Source implementation plan: `plans/implementation/windows-native-sandbox/036-mxc-denial-capture-provenance-corrective.md`

Source roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0024-capture-and-handle-semantics-follow-up`

Implementation evidence SHA: `1803105fbf2434eaaf9ba673f77e2b124c4eb151`.

Native evidence: [Windows MXC C036 run 38429625037](https://github.com/dbowm91/codegg/actions/runs/38429625037). The job completed successfully as evidence collection; its report sets `qualified_test_only=false` because the actual child denial was absent from the captured event document and the timeout child was not confirmed gone.

## Finding

The pinned MXC v1.0.0 API returns `ExecutionResult.output_metadata.capture_denials`, including the actual stamped output path. The configured `denials.json` is not the literal output filename. The C036 runner followed that metadata pointer, verified the file stayed inside the owned capture directory, parsed the bounded schema, and matched its summary fields. This resolves the caller-path assumption in C035: the capture document was present and attributable to the run, so the original missing-file observation did not establish capture unavailability.

The Windows child reached its nonce-bound entry and stage proof, and its open of the same synthetic host file returned `ERROR_ACCESS_DENIED` (`5`). The ordinary inherited-handle positive control read the known bytes from the same file. The block-mode capture document referenced by the terminal metadata had a valid schema and summary but `totalDenials=0`; it contained no event correlated to the child PID and file read. In this final exact-SHA run, the deliberately timed child entered and timed out, but the runner could not confirm its process was gone afterward (`timeout_process_gone=false`). Therefore C036 does **not** qualify per-child denial telemetry or bounded timeout cleanup, even though the path operation itself was denied.

## Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Pinned SDK provenance | `Cargo.lock` checksum `e4866ca589a68edc8e931eba58eed9253ad1ce670e974aedc4830a1d94bca6cd`; crate VCS commit `a846837d827b496fc4a08dfe60554515758c6647`, path `src/mxc-sdk`; terminal `ExecutionResult.output_metadata.capture_denials` | pass |
| Eligible native host and helper | Run `38429625037`: Windows 11 Enterprise 25H2, build `26200`, UBR `9457`, ARM64; native helper and ARM64 cross-check steps passed | pass |
| Actual selected tier | SDK probe: `base-container`; `baseContainerApiPresent=true`, `baseContainerSupportsDenyPaths=true`, `nativeCaptureAvailable=true` | pass for the observed host only |
| Ordinary read positive | Report `ordinary_control`: exit 0, child PID 10864, same-file handle identity true, `secret_match=true`, path read allowed | pass |
| MXC child entry and host path denial | Report child PID 8012, nonce-bound entry, `Exited(0)`, host-path `ERROR_ACCESS_DENIED`; synthetic content hash unchanged | pass for the tested path operation |
| Capture metadata path and document provenance | Metadata points to `<capture-block>/denials.1068_3a8c2ec90d0d040f37f02813a068fae1.json`; filename has current process ID and run nonce; confined regular file; valid summary, exit code and bounded schema | pass |
| Correlated child denial event | Referenced document summary `totalDenials=0`; `matched_denial_count=0`; no event has the child PID, requested file, and read operation | fail; decisive strict-positive gate |
| Allowed/no-event and disabled/stale controls | Allowed stage proof and capture document validated with no synthetic host false positive; capture-disabled run returned no metadata despite a stale decoy | pass |
| Pre-entry and timeout classification | Pre-entry returned `Exited(23)` and report classifies it `not_run`, not as a denial; timed child entered and returned `TimedOut`, but its PID was not confirmed gone | incomplete; bounded cleanup gate failed |
| Temporary artifacts and fixture integrity | No ETL or MXC capture temp files remained; synthetic file SHA and post-grant ACL snapshot were stable | pass |
| Strict-positive qualification | `C036_REPORT.qualified_test_only=false`; failed gates `child-correlated-denial-event` and `bounded-timeout-classification` | fail; no admission |

The report and raw compact/verbose synthetic capture artifacts are retained for seven days in the linked run artifact. Host paths are deliberately not reproduced here.

## Verification

On the implementation branch, the isolated harness formatting, C036/C037 contract tests, ARM64 Windows cross-check, sandbox contract guard, execution-ownership guard, `git diff --check`, and `bash scripts/verify.sh quick` passed. The exact-SHA Windows run passed pinned tests, ARM64 cross-check, helper build, native execution, and artifact upload. An earlier Windows rustfmt check failed because the runner checkout used Windows newline style; formatting remains checked locally, and the redundant Windows formatting step was removed. The failed formatting-only attempts did not reach native execution.

## Security and disposition

- No production launcher/backend, `SandboxRequest::Required`, installer, schema, ACL policy, network policy, or authority surface changed.
- Only the disposable synthetic fixture received broad-read ACL entries; no populated host object was changed.
- The report distinguishes the denied path operation from absent capture events. It does not infer that missing child telemetry means a capture subsystem failure or that a valid event was suppressed.
- C036 resolves capture output-path provenance but fails the required event-correlation gate and does not confirm timeout-child cleanup in the final run. C037 was independent; C038 subsequently closed with no candidate after reviewing both closures.
- Windows constrained execution remains unavailable/fail-closed. No filesystem or network `Enforced` claim is made; C011/M004–M006/C015–C018 remain blocked and C030/C031 remain terminally blocked.
