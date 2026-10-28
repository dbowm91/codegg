# H006 — Job-Observable Sustained Corrective Qualification

Status: **blocked before live qualification — offline prerequisites pass locally; exact-head hosted CI and fresh explicit live authorization remain**

Implementation: branch `plans/harness-lab-h006-job-observable-20261028`.
Prior live evidence: H005 remains unqualified and its authorization remains consumed (`005-status.md`).
No external model or provider request was made for H006.

## 1. Offline job lifecycle evidence

The native ACP test uses a built CodeGG binary and trusted sandbox helper, submits a real four-crate offline Cargo workspace as a CodeGG background test job, records the exact returned ID, and queries that same ID twice as `running`. Between those observations the deterministic session makes two productive Rust source edits. It then waits on the same ID and observes `completed`. The sanitized receipt records seven native tool calls, nine local fixture-provider turns, 7,713 ms wall time, zero observed ACP event gaps, and no raw tool output or workspace contents.

Receipt: [`006-offline-job-observation.json`](006-offline-job-observation.json)

The composed check exposed and corrected two issues before passing: job-control results are native JSON (with typed error text for failed jobs), and the managed test runner requires the trusted sandbox helper beside the CodeGG binary. The projection now accepts bounded JSON fields plus the native error-text status form, keeps absent or unrecognized fields explicit as `null`, and never retains raw output. CI builds both binaries and stages them as siblings for the native test.

Exact commands:

```text
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/tmp/codegg-h006-native-target RUSTC=/Users/davidbowman/.rustup/toolchains/1.89-aarch64-apple-darwin/bin/rustc RUSTDOC=/Users/davidbowman/.rustup/toolchains/1.89-aarch64-apple-darwin/bin/rustdoc /Users/davidbowman/.rustup/toolchains/1.89-aarch64-apple-darwin/bin/cargo build --locked --bin codegg --bin codegg-sandbox-helper
CODEGG_H006_CODEGG_BINARY=/tmp/codegg-h006-native-target/debug/codegg CODEGG_H006_JOB_RECEIPT=plans/closure/harness-lab-mainline-qualification/006-offline-job-observation.json python3 -m unittest dev.harness_lab.tests.test_sustained.LongHorizonNativeJobTest.test_h006_real_background_job_is_observed_by_exact_id_while_work_continues
```

Result: build succeeded; native ACP lifecycle test passed in 8.237 seconds.

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

H006's offline prerequisites are complete locally. Exact-head hosted CI is pending. H006 is not a sustained autonomous qualification: there was no external model call, no live provider request, and no budget authority inferred from H005. Any live attempt still requires separate explicit approval naming the `opencode_go` route, request and time caps, disk stop floor, cost handling, and cancellation conditions.
