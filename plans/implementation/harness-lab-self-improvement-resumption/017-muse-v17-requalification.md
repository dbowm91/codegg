# S017 — Muse Spark v17 H007 requalification

Status: **authorized; 1 of 20 runs started**

## Objective

Use `opencode_go/muse-spark-1.3-contributor` on versioned H007 task prompts, starting with v17 to determine whether the frozen-manifest correction resolves task acceptance and whether the additional crash-safe checkpoint work supports a 30-minute substantive horizon. Preserve the independent verifier, exact-ID lifecycle, seeded repairs, continuation, and all H007 acceptance gates.

## Authorization

The user authorized up to 20 sequential live attempts on 2026-11-03. The exact route is `opencode_go/muse-spark-1.3-contributor`; v17 manifest SHA-256 is `402b3c1981a3ba5a53a4331a20825e5f6a7fd8479a5de6cae399aa0e618ba4bc`; v18 SHA-256 is `2558b3d03f0adc9b272462c2aae27e07bae4d4e292a8091996fb8646e0485589`. Each run is capped at 750 provider requests and 3,600 seconds, with a 10 GiB free-disk floor and 300-second activity-stall cancellation. Cancel on operator request, sensitive exposure, or loss of request-admission observability. Record provider USD cost when available and `unknown` otherwise. No other provider/model is authorized. The append-only ledger is `plans/closure/harness-lab-self-improvement-resumption/evidence/s017-muse-live-authorization-20261103.json`.

## Method

Run one case at a time using the current recorded manifest revision and the existing `opencode_go` adapter. Every started live run consumes one of 20 slots. Record sanitized task/verifier/horizon outcomes, exact job lifecycle, repair count, continuation, request count, event pairing, activity gaps, disk minimum, and cost status. Do not inspect private verifier source or output. If a run fails, attribute only what safe receipts establish; make a narrow offline correction only when evidence supports it, version the prompt, and continue within the same 20-run authorization.

Qualification requires task verifier pass and all unchanged H007 gates, including at least 30 minutes of substantive activity, no gap over five minutes, exact-ID job running/terminal with productive overlap, both repairs, continuation, complete event capture, and no infrastructure failure. Completing the task correctly before 30 minutes is task success but not horizon qualification.

## Stop

Stop on the authorized caps, cancellation triggers, or 20 started runs. Do not claim qualification without a passing independent verifier and all horizon gates. S016's consumed DeepSeek authorization remains separate and is not reopened.

## Attempt 1 and corrective

Attempt 1 used v17 on Muse Spark. It ended normally after 26m36s active, with 233/750 provider requests. Exact-ID running/terminal observation and productive overlap passed; continuation and 18 repair cycles were recorded; event capture was complete, maximum activity gap was 59.759 seconds, and minimum free disk was 220,531,945,472 bytes. The verifier ran five tests and reported one failure. The 30-minute horizon was short by about 3m24s.

The lane's `fixture/README.md` hash was `71f1b2de...`, matching the stale `dev/autonomy_fixtures/build-provenance-catalog/fixture/README.md` source used by the runner, while the corrected metadata expected the synchronized starter hash. The subject did not establish a verifier pass. The single failure is likely a fixture-staging identity mismatch; private verifier output was not inspected. The actual runtime fixture source is now synchronized, and the existing package-equality test plus the frozen-hash regression both pass. Prompt v18 keeps all frozen inputs read-only and adds a dependent end-to-end reconcile/restart workflow. It is prepared for attempt 2 under the same authorization.

Attempt 2 used v18 and passed the independent verifier (5/5). It completed normally after 18m46s active, so task correctness passed but the horizon was `not_observed` (`task_completed_before_threshold`). Exact-ID job overlap, continuation, nine repairs, and event capture passed; there were no runtime, stall, request, or disk failures. The run used 231/750 requests, reported $0.090256, and retained about 221 GiB free. Evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s017-run-02-summary.json`.

Prompt v19 adds two dependent product capabilities—reverse-impact planning and digest-bound read-only policy evaluation—to extend substantive engineering work. Manifest SHA-256: `4cf5821c13b95ab073a8a2ba85498fddf70dd6bbeca8301de9ab436001512ab5`.

Attempt 3 used v19. The agent completed all 14 recorded phases and a 49-test release workspace run, but the hidden verifier reported one failure. Frozen hashes all matched. Public review found that `catalog-query::diff` changed added/removed row formats from the v18/reference contract; this is a plausible cause, not confirmed from private verifier output. The designated release workspace job was deferred to the end; the event trace shows it reached `running` and `completed` with no productive edit between, so the job-overlap criterion failed. Active work lasted 28m56s, 64 seconds short; event capture and disk remained healthy. Evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s017-run-03-summary.json`.

Prompt v20 explicitly freezes the original CLI row formats, moves the designated job sequence before `catalog-delta`, and requires two productive edits between `running` and terminal observation. It also adds incremental bounded impact views against the 250,000-package index. Manifest SHA-256: `b56d958281992f3079b40f055445adde940e6cd6e5f7e91f40448248806e4e4b`.


Attempt 4 used v20 and exceeded 30 minutes of active work, but did not qualify: the priority job was running during two edits and then failed 20/1; the final workspace sweep passed 44 tests. A separate repair-job `wait` blocked for 300,138 ms, creating an activity gap 138 ms beyond the five-minute ceiling. The verifier exited after 240,465 ms with zero tests and one setup error. The private output was not inspected and the cause remains unknown.

The receipt also exposed an evidence attribution gap: `manifest_sha256` names the fixture's canonical H007 manifest instead of the selected campaign manifest. The runner now adds `requested_manifest_sha256`, propagated from the loaded suite. Focused provenance and refusal tests pass.

Prompt v21 bans blocking job waits, requires nonblocking status polls, requires known test failures to be repaired before the one qualifying job, and explicitly records the prior verifier setup error as unresolved. Manifest SHA-256: `11370f39d0acc75b90363ad340b116ab0f9dd7bf9760b91faae2e76e3cb070f6`.


Attempt 5 used v21 and made over 43 minutes of tool progress, with 636 complete calls and no event gaps. It then continued to admit provider requests through request 745/750 while making no native tool calls for more than 300 seconds. I cancelled the blocked ACP turn under the authorized stall rule. The runner's watcher had treated all `session/update` notifications as activity, so streamed assistant text hid the tool stall. The verifier did not run; task outcome is inconclusive.

The corrective now aligns stall detection with the qualification metric by counting only native tool start/terminal events, and qualification measures the final silent interval as well. Focused watcher and trailing-gap regressions pass. Prompt v22 makes this tool-progress boundary explicit. Manifest SHA-256: `ccbe0d8585c3bd6a965ad0a0a89c8e1e34c9681758e9856817bca6b760b4b128`.


Attempt 6 is the first run to meet the sustained horizon and exact-job gates together: 53m49s active, 22 repairs, the designated job completed with two productive edits during its running state, full continuation, and no event gaps. The final release workspace suite passed 45 tests. The independent verifier still exited at 240,408 ms with zero tests and one error; its private output was not inspected. A fresh public debug workspace sweep remained in the frozen impact-scale test for over seven minutes before operator cancellation, while the task's targeted release scale run passed. A verifier debug setup timeout is likely but unconfirmed.

Prompt v23 directs the model to profile and optimize the public impact-view implementation so the unchanged frozen debug workspace suite can complete within the observed verifier window, without changing tests or verifier. Manifest SHA-256: `c644badea0601ace04765acf3ed7bfc9e8628dfed40532535f1d93b9150d842e`.
