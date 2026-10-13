# Windows Native Sandbox C023 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/windows-native-sandbox/023-launcher-ci-historical-negative-reconciliation.md`

Source subsystem roadmap:

- `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0020-corrective-qualification-after-c022`

Repository baseline reviewed: `68113300c68961a57faa6902ef6307b5e5e59db9` (exact C023 implementation and required CI SHA)

Implementation commits or pull requests:

- `68113300` — separate C014's known-negative experiment from required launcher CI, remove duplicate invocations, and retain the experiment as explicit opt-in research.
- `40de0257` — record Windows runner UBR in the required workflow metadata.
- Required Windows launcher workflow [38359288381](https://github.com/dbowm91/codegg/actions/runs/38359288381) passed on exact SHA `68113300c68961a57faa6902ef6307b5e5e59db9`.
- Opt-in C014 reproduction run [38359300786](https://github.com/dbowm91/codegg/actions/runs/38359300786) reproduced the expected Git `/dev/null` failure; its independent `process-launcher` job passed. The research failure is not a launcher CI pass.
- Follow-up Windows launcher workflow [38362180227](https://github.com/dbowm91/codegg/actions/runs/38362180227), SHA `40de0257b1fde353c2b2b3f9c71a51fd7000bb7f`, records the UBR and repeats the required suite.

## 1. Executive finding

C023 is closed. The mandatory Windows launcher job now runs the previously skipped positive launcher, Job, ACL, environment, status, AppContainer-ordering, and shell regressions without the known-negative C014 Git experiment gating them. No blanket `continue-on-error` was introduced. C014 remains available as a clearly labeled manual research job and still reports its historical negative result.

The required run on the implementation SHA passed. The follow-up run records Windows Server 2025 Datacenter version `10.0.26100`, build `26100`, UBR `33438`; its metadata step completed and the full repeated launcher suite is the run associated with the UBR update. The earlier exact implementation run reported edition/version/build but did not yet print UBR.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| C014 no longer blocks required positive tests | Required run `38359288381`, exact SHA `68113300...` | pass | All 18 process-launcher workflow steps completed successfully. |
| Positive native test selections are nonzero | Required run logs; 48 selected tests across 12 focused invocations | pass | Counts listed below; all selected cases passed. |
| Duplicate invocations removed without losing distinct suites | `.github/workflows/windows-process-launcher.yml` and run logs | pass | One each of launcher, suspended-target, restricted-stdio, environment, status, managed integration, ACL integration, adversarial, ACL unit, AppContainer ordering, policy, and six distinct shell tests. |
| C014 historical failure retained as explicit research | Run `38359300786`, `c014-negative-reproduction` job | pass as research reproduction | The expected job failed at Git init due to AppContainer `/dev/null` denial; its separate required launcher job passed. |
| Host metadata and runtime | Run `38359288381`; follow-up `38362180227` metadata step | pass | Windows Server 2025 Datacenter, version `10.0.26100`, build `26100`, UBR `33438`; Rust `1.89.0`, `x86_64-pc-windows-msvc`. |
| Product sandbox admission unchanged | Source diff and workflow scope | pass | No production backend, `Required`, filesystem/network `Enforced`, or ACL policy changed. |

Selected test counts in the exact implementation run:

| Focused invocation | Selected | Passed | Failed / ignored |
|---|---:|---:|---:|
| `windows_process_launcher` | 8 | 8 | 0 / 0 |
| Suspended-target Job assignment failure | 1 | 1 | 0 / 0 |
| Restricted stdio setup failure | 3 | 3 | 0 / 0 |
| Managed environment block | 1 | 1 | 0 / 0 |
| Status-frame rejection | 1 | 1 | 0 / 0 |
| Restricted managed process integration | 6 | 6 | 0 / 0 |
| Journaled ACL lease/recovery integration | 4 | 4 | 0 / 0 |
| Existing-root adversarial qualification | 1 | 1 | 0 / 0 |
| ACL lease unit tests | 6 | 6 | 0 / 0 |
| AppContainer launch ordering | 5 | 5 | 0 / 0 |
| Windows policy input tests | 6 | 6 | 0 / 0 |
| Six distinct native shell tests | 6 | 6 | 0 / 0 |
| **Total selected** | **48** | **48** | **0 / 0** |

## 3. Production implementation evidence

Changes are limited to `.github/workflows/windows-process-launcher.yml` and `architecture/testing.md`. No production launcher, backend, scheduler, ACL mutation, API, config, storage, or protocol changed. The manual C014 job remains opt-in and separate from required launcher regressions.

## 4. Verification executed

### Commands and hosted evidence

```text
cargo fmt --all -- --check
python3 scripts/check_sandbox_contract.py
python3 scripts/check_execution_ownership.py
bash scripts/verify.sh quick
GitHub Actions: Windows process launcher, run 38359288381
GitHub Actions: manual C014 negative reproduction, run 38359300786
GitHub Actions: UBR metadata and repeated launcher suite, run 38362180227
```

### Results

- Local formatting, sandbox contract, execution ownership, and `verify.sh quick` passed.
- Run `38359288381` passed all required process-launcher workflow steps at the exact C023 implementation SHA. Each required test invocation selected at least one case; the 48 selected cases all passed.
- Run `38359300786` had the expected failed C014 research job (`git init` exit 128, `/dev/null` permission denial) and a successful, independent process-launcher job. The top-level workflow is red because of the expected research finding; this is explicitly not a required CI pass.
- Run `38362180227` records UBR `33438` and repeats the required test suite after metadata-only workflow refinement. C014 was skipped on this push-triggered run by design.
- No Windows 11 host compatibility, host-denial, or broader security claim is made by C023.

## 5. Invariant review

- C014's known-negative test was not deleted, skipped unconditionally, or converted into a false pass.
- No blanket `continue-on-error` was added.
- Genuine required launcher failures still fail the job.
- Production Windows `SandboxRequest::Required` remains unavailable; filesystem/network `Enforced` remains unclaimed.
- No original workspace ACL or test fixture ACL policy changed as part of C023.

## 6. Failure and recovery review

The optional C014 job fails at the known Git `/dev/null` denial and makes that research result visible. The independent required launcher job continues and passed. The required job uses the existing disposable hosted runner and focused serial test invocations where required; no persistent host resources are created by this workflow change.

## 7. Migration and compatibility review

No API, protocol, storage, config, MSRV, or runtime behavior changed. Workflow uses Rust `1.89.0` on Windows Server 2025. No Windows 11 equivalence is inferred.

## 8. Security review

This is a CI truthfulness correction, not a security-boundary change. It improves regression visibility while preserving C014's negative evidence. C011/M004 qualification remains separate and blocked; C023 does not establish filesystem or network isolation.

## 9. Documentation and operations

- Updated `architecture/testing.md` to classify C014 as explicit research and distinguish its expected failure from required launcher CI.
- Updated the workflow to expose `run_c014_research` as an opt-in dispatch input and to log edition/version/build/UBR.
- Updated roadmap, registry, and the C023–C026 assessment with exact run IDs and outcomes.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | C014 Git initialization still fails under its historical AppContainer projection. | The ADR-0019 projection path remains a negative candidate. | Preserve the separate negative closure; do not make it a required positive gate or a pass. |
| medium | Passing launcher CI does not qualify C011/M004 or a production sandbox. | Windows constrained execution remains unavailable. | Continue to honor the separate security and admission gates. |

## 11. Roadmap disposition

C023 is closed and makes launcher CI a truthful regression gate. It unblocks no security capability by itself. C024 closed without a viable restricted-token launch candidate; C025 was therefore abandoned and C026 records no admission. C011/M004–M006 and C015–C018 remain blocked.

## 12. Registry updates

- Mark C023 closed with runs `38359288381`, `38359300786`, and `38362180227`.
- Record required positive suite counts separately from the expected negative C014 research job.
- Preserve Windows production unavailability and all independent security gates.
