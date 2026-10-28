# H006 — Job-Observable Sustained Corrective Qualification

Status: **offline prerequisites and prior exact-head CI passed; first authorized live attempt failed during managed-test startup; paired-runfile preflight implemented and locally verified; corrective hosted CI pending; sustained qualification remains incomplete**

Implementation: branch `plans/harness-lab-h006-job-observable-20261028`.
Prior live evidence: H005 remains unqualified and its authorization remains consumed (`005-status.md`).
The initial H006 closure covered offline evidence only. A separately authorized live attempt and its failed result are recorded in §7 below.

## 1. Offline job lifecycle evidence

The native ACP test uses a built CodeGG binary and trusted sandbox helper, submits a real four-crate offline Cargo workspace as a CodeGG background test job, records the returned ID, and uses a bounded wait while the job runs. It then makes two productive Rust source edits, observes that same ID twice as `running` with another productive edit between observations, and waits on the same ID to `completed`. The observation is output-derived: H006 parses bounded job ID/status metadata from native ACP tool output. This proves neither a separate automatic terminal notification nor a new structured job-lifecycle event protocol. The sanitized hosted receipt records exact ID `74386df4-54d6-41eb-b2fc-0f52ade38983`, eight native tool calls, ten local fixture-provider turns, 24,459 ms elapsed, zero observed ACP event gaps, and no raw tool output or workspace contents. The receipt was uploaded by exact-head CI run [40666897399](https://github.com/dbowm91/codegg/actions/runs/40666897399) at source commit `c838def96` and is committed below.

Receipt: [`006-offline-job-observation.json`](006-offline-job-observation.json)

The composed check exposed and corrected two issues before passing: job-control results are native JSON (with typed error text for failed jobs), and the managed test runner requires the trusted sandbox helper beside the CodeGG binary. The projection now accepts bounded JSON fields plus the native error-text status form, keeps absent or unrecognized fields explicit as `null`, and never retains raw output. CI builds both binaries and stages them as siblings for the native test.

Hosted commands:

```text
cargo build --locked --bin codegg --bin codegg-sandbox-helper
python3 -m unittest discover -s dev/harness_lab/tests
```

Result: native build succeeded; the full Harness Lab Python suite passed, including the exact-ID running/terminal assertion. CI uploaded the sanitized receipt artifact. The latest full exact-head CI run [40671109058](https://github.com/dbowm91/codegg/actions/runs/40671109058) passed at `009d56b0` (Clippy and workspace tests included); the earlier implementation-source run [40666897399](https://github.com/dbowm91/codegg/actions/runs/40666897399) also passed at `c838def96`.

## 2. Request-cap proof

The child-process test invokes the real OpenCode Go provider boundary against a local loopback server with cap `N=2`. Two requests reach transport and receive local fixture responses; request three returns `request_budget` without reaching transport. The test checks the server received exactly two requests and the complete admission receipt contains request numbers 1 and 2 with limit 2. The policy binding tests also prove a trial-supplied cap reaches the child through `SubjectSpec.environment()` without an inherited parent cap. The sustained event-ledger case now refuses to start if the provider cap is absent.

Commands:

```text
CARGO_BUILD_JOBS=2 cargo test -p codegg-providers --lib opencode_go::capture_tests::provider_request_cap_refuses_before_transport_and_writes_complete_receipt -- --nocapture
python3 -m unittest dev.harness_lab.tests.test_supervisor dev.harness_lab.tests.test_autonomy_run
```

Recorded results: provider-boundary child-process test passed (1 test); Python binding and refusal checks passed in the focused suite. A later full focused run passed 93 tests with 1 skipped.

## 3. Hidden-verifier calibration

The hidden verifier was run without changing the four-crate fixture or its verifier. An independently completed solution passed all five checks, including the 50,000-row / five-shard scale oracle, tests, end-to-end journal and CLI behavior, transcript overlap and repair cycles, and frozen hashes. The untouched starter failed the workspace and end-to-end implementation checks while the transcript-shape and frozen-input controls still passed. This distinguishes an implementation failure from a measurement gap; it does not constitute a live model qualification.

Calibration used isolated copies under `/tmp/codegg-h006-ledger-calibration-run` and `/tmp/codegg-h006-ledger-unsolved-run`, with local Cargo targets. Both verifier outcomes were observed before beginning H006 native qualification.

## 4. Focused checks and resource record

- `python3 -m unittest dev.harness_lab.tests.test_autonomy_faults dev.harness_lab.tests.test_autonomy_run dev.harness_lab.tests.test_supervisor`: **94 passed, 1 skipped** after the projection and cap changes.
- Projection-specific native JSON, submission, missing-field, and native error-status tests: **4 passed**.
- `python3 scripts/check_test_self_reexec_paths.py`: passed.
- `python3 scripts/check_test_assertion_quality.py`: passed.
- `python3 scripts/check_test_duplication.py`: passed.
- `python3 -m py_compile dev/harness_lab/supervisor.py dev/harness_lab/tests/test_supervisor.py dev/harness_lab/tests/test_sustained.py`: passed.
- `git diff --check`: passed.
- Hidden-verifier positive calibration: **5 passed**. Negative calibration: expected implementation failures on untouched starter; transcript and frozen-input controls passed.
- Lowest observed free disk during this work: **14 GiB**. The explicit live stop floor remains 10 GiB; no run approached that floor. Temporary build and calibration artifacts are to be removed before handoff.

## 5. Qualification boundary

H006's offline prerequisites include hosted evidence for the composed native job lifecycle, sanitized receipt, request-cap propagation/enforcement, and passing exact-head CI. Job state is projected from bounded native ACP tool output; a separate automatic terminal notification and a structured job-lifecycle event protocol remain unproven and out of H006's demonstrated result. The first live attempt failed and did not establish sustained autonomous qualification; see §7. A further attempt requires separate explicit approval naming the `opencode_go` route, request and time caps, disk stop floor, cost handling, and cancellation conditions.

## 6. Exact-head corrective follow-up

At `36916de8710fc5f3d409ef8b9b12c84be9dc5fb1`, the H006 test requires the exact job to reach `completed`, with a negative case proving `failed` cannot qualify. The native sandbox probe denies Rustup and Cargo credential locations, unrelated Rustup data, an unapproved path, and a symlink escape. Rustup `credentials` and `credentials.toml` are included in sensitive denies for default and overridden Rustup homes. The Windows allowance path helper now returns validated ordinary paths so the sandbox resolver can reject device-prefix input without rejecting its own canonicalized allowance output.

All PR-event checks passed on this exact head: [Verify](https://github.com/dbowm91/codegg/actions/runs/40730023534), [macOS qualification](https://github.com/dbowm91/codegg/actions/runs/40730023357), [Windows process launcher](https://github.com/dbowm91/codegg/actions/runs/40730023470), [Linux process/IPC and restricted-userns](https://github.com/dbowm91/codegg/actions/runs/40730023356), and [protected-mount](https://github.com/dbowm91/codegg/actions/runs/40730023403). The implementation PR qualified offline. The later live attempt and its failure are recorded in §7.

## 7. First live attempt — failed, not sustained-qualified

On 2026-10-28, one separately authorized H006 attempt ran from source commit `85fd548c32af15195d8fb267d528721b8295b139` with route `opencode_go/muse-spark-1.3-contributor`, a 120-request ceiling, a 60-minute hard stop, and a 10 GiB free-disk floor. The session ended naturally after 620,751 ms (10m 21s) and 27 provider requests. USD cost was unreported (`null`); the lowest observed free space was 64,463,257,600 bytes and the floor did not trigger.

The supplied `codegg` executable lacked its trusted sibling `codegg-sandbox-helper`. Managed Rust test jobs therefore failed before Cargo started. The job receipt reported the exact queried job as `failed`; no successful `running` state with productive overlap was recorded. ACP observation was complete (38 tool starts, 38 terminals, zero event gaps), and daemon cleanup succeeded. The agent repeated the same failing test action and recorded only one repair cycle. The independent hidden verifier exited 1. H006's 30-minute active-work, overlapping-job, two-cycle, and verifier-pass criteria were not met.

The corrective owner is H006 lane preparation and autonomy-runner preflight. The canonical binary must be built together with `codegg-sandbox-helper`, this corrective branch requires the native `doctor installation` report to show the trusted helper present before reserving provider capacity. A narrowly scoped runtime watcher also cancels if that specific unrecoverable helper-resolution error appears during a live session. The missing helper was built after the attempt with the pinned Rust 1.89 ARM64 toolchain, and the native report confirmed it present. The focused autonomy/fault/supervisor suites passed (100 tests, 1 skipped); the complete Harness Lab Python suite passed (380 tests, 9 skipped) with `CODEGG_H006_CODEGG_BINARY` set to the paired binary. Test assertion-quality and duplication guards passed. Hosted CI for this corrective branch remains pending. The original authorization is consumed; another live attempt requires fresh explicit authorization after the corrective hosted checks.
