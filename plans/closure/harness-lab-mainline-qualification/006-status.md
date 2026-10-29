# H006 — Job-Observable Sustained Corrective Qualification

Status: **offline prerequisites and paired-runfile preflight passed; two separately authorized live attempts failed qualification; the latest ended before 30 minutes and its unchanged verifier failed; no live authorization remains**

Implementation: branch `plans/harness-lab-h006-job-observable-20261028`.
Prior live evidence: H005 remains unqualified and its authorization remains consumed (`005-status.md`).
The initial H006 closure covered offline evidence only. The first authorized live attempt and its failure remain recorded unchanged in §7. The fresh 2026-10-29 attempt is recorded in §8; its authorization is consumed as well.

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

H006's offline prerequisites include hosted evidence for the composed native job lifecycle, sanitized receipt, request-cap propagation/enforcement, and passing exact-head CI. Job state is projected from bounded native ACP tool output; a separate automatic terminal notification and a structured job-lifecycle event protocol remain unproven and out of H006's demonstrated result. Both live attempts failed and did not establish sustained autonomous qualification; see §§7–8. Any further attempt requires separate explicit approval naming the `opencode_go` route, request and time caps, disk stop floor, cost handling, and cancellation conditions.

## 6. Exact-head corrective follow-up

At `36916de8710fc5f3d409ef8b9b12c84be9dc5fb1`, the H006 test requires the exact job to reach `completed`, with a negative case proving `failed` cannot qualify. The native sandbox probe denies Rustup and Cargo credential locations, unrelated Rustup data, an unapproved path, and a symlink escape. Rustup `credentials` and `credentials.toml` are included in sensitive denies for default and overridden Rustup homes. The Windows allowance path helper now returns validated ordinary paths so the sandbox resolver can reject device-prefix input without rejecting its own canonicalized allowance output.

All PR-event checks passed on this exact head: [Verify](https://github.com/dbowm91/codegg/actions/runs/40730023534), [macOS qualification](https://github.com/dbowm91/codegg/actions/runs/40730023357), [Windows process launcher](https://github.com/dbowm91/codegg/actions/runs/40730023470), [Linux process/IPC and restricted-userns](https://github.com/dbowm91/codegg/actions/runs/40730023356), and [protected-mount](https://github.com/dbowm91/codegg/actions/runs/40730023403). The implementation PR qualified offline. The later live attempt and its failure are recorded in §7.

## 7. First live attempt — failed, not sustained-qualified

On 2026-10-28, one separately authorized H006 attempt ran from source commit `85fd548c32af15195d8fb267d528721b8295b139` with route `opencode_go/muse-spark-1.3-contributor`, a 120-request ceiling, a 60-minute hard stop, and a 10 GiB free-disk floor. The session ended naturally after 620,751 ms (10m 21s) and 27 provider requests. USD cost was unreported (`null`); the lowest observed free space was 64,463,257,600 bytes and the floor did not trigger.

The supplied `codegg` executable lacked its trusted sibling `codegg-sandbox-helper`. Managed Rust test jobs therefore failed before Cargo started. The job receipt reported the exact queried job as `failed`; no successful `running` state with productive overlap was recorded. ACP observation was complete (38 tool starts, 38 terminals, zero event gaps), and daemon cleanup succeeded. The agent repeated the same failing test action and recorded only one repair cycle. The independent hidden verifier exited 1. H006's 30-minute active-work, overlapping-job, two-cycle, and verifier-pass criteria were not met.

The corrective owner is H006 lane preparation and autonomy-runner preflight. The canonical binary must be built together with `codegg-sandbox-helper`; the runner requires the native `doctor installation` report to show the trusted helper present before reserving provider capacity. A narrowly scoped runtime watcher cancels if that specific unrecoverable helper-resolution error appears during a live session. The first watcher implementation expected the tool name on `tool_call_update`, but production ACP publishes the title only on `tool_call` and correlates the terminal update by `toolCallId`. The watcher now records `job`/`test` starts and checks the matching failed terminal event. A production-shaped event-sequence regression omits `title` from the terminal update and verifies cancellation; negative sequences cover unrelated tools, unrelated failures, nonterminal updates, and orphan terminals.

The missing helper was built after the attempt with the pinned Rust 1.89 ARM64 toolchain, and the native report confirmed it present. On the rebased corrective branch, the focused watcher tests passed (2 tests), the complete Harness Lab Python suite passed (392 tests, 9 skipped) with `CODEGG_H006_CODEGG_BINARY` set to the paired binary, and test assertion-quality, test duplication, `py_compile`, and `git diff --check` passed. The exact watcher corrective commit is `84f16820403ae9519c52ea4d8ccb9c0be8d3ca41`; hosted [Verify passed in 32m03s](https://github.com/dbowm91/codegg/actions/runs/40812630328), including the Harness Lab Python suite and workspace tests, and [restricted-userns qualification passed in 17m16s](https://github.com/dbowm91/codegg/actions/runs/40812624396). The separate Linux process/IPC workflow was triggered by the rebase push, although this PR's changed paths are limited to Harness Lab code and documentation. It reached the final constrained LSP/procfs stage after earlier probes passed, then hit that workflow's 45-minute timeout at 45m08s; no process, IPC, LSP, or sandbox Rust source is changed by this corrective. PR #214 merged at `a14ce0da2fc00879db8b5dcdabf58d8da6c83cda`. The first live authorization was consumed by §7; a fresh authorization was used by §8 and is now consumed as well.

## 8. Second live attempt — failed, not sustained-qualified

On 2026-10-29, after PR #214 merged, one fresh authorization covered one `opencode_go/muse-spark-1.3-contributor` attempt with a 120-request ceiling, a 60-minute hard stop, a 10 GiB disk floor, and a 30-minute active-work minimum. The paired `codegg` and `codegg-sandbox-helper` binaries were built together from `origin/main` `a14ce0da2fc00879db8b5dcdabf58d8da6c83cda`; their SHA-256 digests are recorded in the invocation evidence. The runner correction at `d7863245b65e9bf0f5713b4ceaaa6c3c38f1fc45` made a 55-minute turn timeout available and bounded verifier time by the remaining deadline. The fixture and hidden verifier were unchanged from the original H006 source.

The provider session ended naturally after 600,432 ms (10m 00s) and 34 provider requests, using the full-session budget receipt. The route reported a provider allowance of USD 0.017106; no independently verified billed amount is asserted, so actual USD cost remains unknown. The lowest observed free space was 154,950,729,728 bytes, above the 10 GiB floor. Neither request nor disk cap triggered. The run had 45 ACP tool starts, 45 terminals, zero event gaps, and clean daemon shutdown. The request-cap admission receipt records all 34 admitted requests. The earlier missing-directory preflight refusal is preserved separately; it was refused before provider reservation and is not counted as a live attempt.

The model made two distinct test-failure/repair cycles; the unchanged verifier's corresponding cycle test passed. Frozen public inputs also passed. However, the model never observed a submitted job in `running`: the exact-ID `job` status observations in the transcript were terminal `completed` results. The job/ACP system recorded no event gaps, and background jobs reached terminal state, so this run does not identify a daemon, scheduler, or ACP event-loss defect. It instead shows the agent checked IDs only after those short test jobs had completed. The agent ended its turn at ten minutes and left an unexpected closing brace in `ledger-store/src/lib.rs:59`; the independent workspace test and CLI checks therefore failed to compile. Active duration, exact-ID running overlap, completion, and verifier-pass criteria were not met.

The unchanged verifier reported three errors and two passes. The repair-cycle and frozen-input checks passed. The workspace and end-to-end CLI checks failed on the Rust syntax error. The overlap test raised `AttributeError: 'NoneType' object has no attribute 'get'` because the harness transcript emits `"job": null` for ordinary tool calls while the verifier uses `call.get("job", {}).get(...)`. This is a measurement-boundary defect distinct from the agent's missing running-state observation; it prevented that verifier assertion from returning a direct pass/fail result. The verifier's source digest remains `b885dacfc60add5eb755004ec57810444fcd61a4a23fa4c8f5eb76eff96c53f9` and was not changed.

Evidence is stored under `evidence/006-live-*20261029.*`. The transcript/verifier and job-overlap follow-up is recorded in §9. Do not attribute this result to ACP event loss or broaden it into a daemon redesign. The 2026-10-29 authorization is consumed; another provider attempt requires a separate fresh approval.

## 9. Offline corrective after the second attempt

The two findings above received focused corrections without changing the failed receipt or making another provider call:

- `_ToolCallProjector.calls()` now omits the optional `job` key for ordinary non-job calls. Job/test calls whose output is unrecognized still carry explicit null job identity/status fields. The hidden verifier source and digest remain unchanged.
- A production-shaped ACP sequence regression builds ordinary read/edit calls, a background test submission, two productive edits, exact-ID `running`, and exact-ID `completed`, then invokes the unchanged hidden overlap verifier. It passes. This proves the verifier now consumes the actual transcript shape; it does not retroactively add a running observation to the failed model trace.
- The task-owned `ledger-index/tests/high_volume.rs` runs 24 deterministic 500,000-record merge passes, including duplicate shards. It completes in 34.76 seconds locally without sleeps. The H006 prompt now schedules this test as the background job while the agent implements dependent CLI stages and explicitly requires status checks on the same ID.
- H006's runner now requires a 30-minute turn duration and an observed tool-call span of at least 30 minutes before marking the case qualified. The 60-minute hard stop remains authoritative.
- The new stress test and README are frozen in case metadata; the existing fixture source tests and hidden verifier were not edited. The earlier attempt's fixture, prompt, and verifier remain recoverable at its recorded source commit.

Verification on the corrective worktree: `python3 -m unittest dev.harness_lab.tests.test_autonomy_run dev.harness_lab.tests.test_autonomy_faults dev.harness_lab.tests.test_autonomy` passed (141 tests); the high-volume Cargo integration test passed (1 test, 34.76 s); the unchanged hidden overlap test passed against the production-shaped sequence; the unchanged frozen-input verifier test passed; test assertion-quality and duplication guards passed; Python compilation, `rustfmt --check`, and `git diff --check` passed. These are local offline results; no hosted CI or live provider qualification was run for this follow-up.

H006 remains unqualified and both live authorizations remain consumed. Any next live run needs a fresh explicit authorization after review of this corrective; the completed offline tests do not satisfy the sustained live criteria.
