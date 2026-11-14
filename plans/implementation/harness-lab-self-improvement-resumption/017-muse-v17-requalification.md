# S017 — Muse Spark v17 H007 requalification

Status: **unqualified; authorization exhausted (20/20 runs)**

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


## Attempt 7 result and next prompt revision

Attempt 7 exceeded the 30-minute active-work threshold, but did not qualify. The exact public failure was a stale Cargo.lock at the priority `cargo test --offline --locked --workspace` job. After a non-locked package check refreshed the lock, the model did not rerun the full locked workspace suite; it left `next_phase: verify_workspace`. The independent verifier did not run. The session later ended on four 120-second OpenCode Go stream-idle failures and retry-chain expiry, with no native-activity stall and complete event capture. Do not classify this as a successful implementation or verifier failure.

Prompt v24 explicitly has the model generate the lockfile offline after all local workspace members/dependencies are registered, run focused and unlocked workspace checks, freeze manifests/lockfile, then execute and observe the designated locked workspace job. It forbids changing manifests after job submission. Provider diagnostics are recorded, but no production provider timeout is widened: the measured stall was upstream and the retry/300-second activity bounds behaved as configured. Continue within the same 20-run authorization, only on the Muse Spark route.


## Attempt 8 result and diagnostic follow-up

Attempt 8 confirms the lockfile correction: the designated priority job passed 19 tests with the two required edits during its running state, and the final locked workspace sweep passed 42. All fifteen phase labels and `next_phase: none` were recorded. The task still failed its independent verifier (one failure among five tests; output sealed) and ended after only 18m13s of active work. It did not qualify and was not a successful early finish.

No verifier-specific cause is inferred. Prompt v25 asks for a public acceptance matrix covering each frozen README/contract item and every added capability, mapping requirements to implementation and assertion-bearing tests; it preserves frozen inputs and hidden-verifier isolation. Continue within the original authorization.


## Attempt 9 result and corrective scope

Attempt 9's independent verifier reported two failures among five tests; output remains sealed. The task ended normally after 25m52s active and therefore missed the horizon. Priority and final locked workspace jobs both passed 42 tests, all phases and continuation were recorded, and event/disk/request gates were healthy.

The public acceptance audit identified two concrete unmet prompt requirements: `ImpactView` still snapshots and lacks incremental upsert/removal against fresh rebuilds at the required 250k mixed sequence; reconcile uses a tautological resume assertion and does not prove restart after every page/checkpoint boundary. These are evidence-based corrective targets, not confirmed names of the hidden failing tests. Prompt v26 targets these gaps and preserves the independent verifier boundary.


## Attempt 10 result and next corrective

Attempt 10 used v26 and was cancelled at the authorized 300-second native-tool stall. It had 163/750 provider requests, 2,187,549 ms elapsed trace time, 158 native tool starts and 157 terminals, with no completed-event gap above 243,493 ms before the final open tool. Free disk stayed above 200 GiB. The last native call was `test`; the hidden verifier did not run and no task result was produced. The runner's cancellation left one orphan executable, which was separately identified by public path and stopped.

Public process evidence showed `catalog-query/tests/impact_scale.rs` running at 100% CPU. The generated test performed `matches_fresh` and a second full `ImpactView::build` for each of 200 mutations against 250,000 records. This is the evidence-backed corrective target: retain the fixed-seed 250k graph and mixed mutation coverage, but compare against fresh rebuilds at bounded checkpoints and avoid repeated full-vector mutation scans so the unchanged debug suite can finish. This is a pathological test workload, not evidence of daemon/provider failure. Attempt 11 started with v27, manifest SHA-256 `d45257b05c31f97a07bc10c1e3c4932af5c2b367b5132ae7e64c57028d9150c0`, under the existing authorization.


## Attempt 11 result

Attempt 11 reused v27. It ended inconclusively on `provider_timeout` after 43/750 requests, with 415 seconds of active tool span and 664 seconds total duration. The trace was complete (44 starts and 44 terminals, no gaps), maximum measured native-activity gap was 97,156 ms, and the disk floor remained healthy. The verifier ran but did not pass; no qualifying priority job, productive overlap, continuation, or horizon was established. Its private output was accidentally exposed during receipt extraction and is excluded from evidence and diagnosis. No task success is claimed. Since the public task never reached the priority lifecycle, attempt 12 repeats v27 to validate the scale-test correction independently of this upstream timeout.


## Attempt 12 result

Attempt 12 repeated v27 and ended on `provider_timeout` after 26/750 admitted requests. It had 388 seconds of active tool span, 31 native starts and terminals with no gaps, a maximum measured activity gap of 128,253 ms, and no disk/stall trigger. The verifier ran zero tests and returned an error; its output remains sealed. No qualifying lifecycle or sustained horizon was established. Attempt 13 will repeat the same manifest to separate a transient provider interruption from the v27 task correction; no timeout or retry-policy changes are justified by this attempt alone.


## Attempt 16 and v28 corrective

Attempt 16 is the first sustained horizon pass: active work exceeded 30 minutes; exact job overlap, continuation, seven repairs, complete events, and disk/activity limits passed. The independent verifier timed out at 240,312 ms with zero tests run, so the task did not qualify. Public source inspection found `ImpactView` mutation scans all live records and recomputes full row/snapshot digests on every update; the 250k mixed test therefore remains too expensive in debug. v28 targets truly incremental per-package/index/digest maintenance while preserving the 250k workload and its ten checkpoints. Attempt 17 uses manifest SHA-256 `6aea30d94b4b10e11e98f0c7cc234393e4e31aea5f6c7a1239ca8e7ea3b5f410`.

## Latest diagnostic evidence

Attempt 17 demonstrates that the configured harness can sustain this task: 48m46s active work, complete native event pairing, a 175,188 ms maximum gap, exact-ID job overlap, continuation, 52 repairs, and a completed five-test verifier run. The task still failed two verifier assertions. The exact categories remain unknown by design; the verifier source and output tail stay sealed. No harness stall corrective is supported by this attempt. Attempt 18 uses the same workload and gates with a single independent public-source/test review before the final locked sweep; it must not add feature breadth or inspect private verifier material.

Attempt 18 used the diagnostic v29 prompt but ended inconclusively after a 240-second first-event wait with zero decoded provider events. The 300-second native activity, request, disk, and event-pairing gates remained healthy; the verifier ran zero tests. No product or harness correction is supported from that run. Attempt 19 repeats v29 to test whether the provider-stream timeout is transient.

Attempt 19 repeated the same v29 provider-stream failure: zero decoded events during the 240,001 ms first-event wait, 12 requests, complete 25/25 event pairing, and no stall/disk failure. This is a second occurrence, but it still does not implicate native tool execution or the 300-second activity watcher. Attempt 20 is the final authorization slot and repeats v29 once more.

## Attempt 20 and corrective closure

Attempt 20 used v29 and ended normally after 10m07s active with 31/750 requests. Event capture was complete (42/42), maximum activity gap was 190,030 ms, minimum free disk was 116,355,694,592 bytes, and neither the activity-stall nor disk guard fired. Exact-ID job overlap was missing and the 30-minute horizon was not observed. The verifier ran zero tests and returned a setup error after 240,367 ms; output remains sealed. S017 did not qualify.

The receipt incorrectly marked continuation true because the old implementation accepted any productive native call after any terminal job. Attempt 20's public continuation file was incomplete (`completed_phases: none`, `next_phase: versioned_records`, `job_id: pending`). The corrected projection reads the bounded public file, requires completed phases and `next_phase: none`, matches a documented completed job ID to a completed native job event, and requires productive work afterward. It rejects symlinks and path escapes. It returns false for attempt 20 and true for attempt 17's complete public continuation. Focused regressions cover the incomplete/failed-job false positive, matching exact job evidence, symlink rejection, and the file-size bound.

The user-authorized 20 live runs are exhausted. Attempt 17 proves the runner can sustain the horizon, but its independent verifier reported two failures; attempt 20 was inconclusive for task correctness and did not meet the horizon gates. No H007 qualification is claimed. Any further live attempt requires fresh explicit authorization. Final safe evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s017-run-20-summary.json`; full authorization ledger: `plans/closure/harness-lab-self-improvement-resumption/evidence/s017-muse-live-authorization-20261103.json`.
