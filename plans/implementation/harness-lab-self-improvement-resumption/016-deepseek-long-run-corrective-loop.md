# Harness Lab Self-Improvement S016 — DeepSeek Long-Run Corrective Loop

Status: **active**

Repository baseline: `300cb4aba` (S015 timeout correction live-confirmed; S015 conditionally closed)

Source roadmap: `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#10-s016-deepseek-long-run-corrective-loop`
Long-term requirements: `plans/000-long-term-specification.md` §§23–29; `plans/001-terminology-and-domain-model.md`
Applicable ADRs: `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md`
Primary class: capability

## 1. Objective

Run the H007 five-crate Rust workload on `opencode_go/deepseek-v4.1-flash` to determine whether CodeGG can sustain a real long-horizon agent task after S015. If a long attempt fails, use a separate bounded live diagnostic run to identify whether the failure is a reproducible CodeGG defect, task/prompt calibration, model behavior, or an unavailable provider route. Correct only an evidence-backed narrow issue, preserve the independent verifier and acceptance criteria, verify the correction, and retry qualification. Stop when the independent qualification passes or 20 total long/diagnostic runs have started.

## 2. Authorization and limits

The user explicitly authorized up to 20 runs on 2026-10-31 and specified DeepSeek 4.1 Flash. The route is `opencode_go/deepseek-v4.1-flash`. This authorization covers the sequence of long and diagnostic runs in this plan; it does not authorize another model or provider adapter.

- Maximum 20 started live runs total; long and diagnostic runs both count.
- Maximum 60 provider requests and 3,600 seconds per run.
- Keep at least 10 GiB free disk; cancel immediately if the floor is crossed or disk state becomes unreadable.
- Run sequentially. Cancel on operator request, cap exhaustion, sensitive-data exposure, or loss of provider-request admission observability.
- Record provider-reported USD when available; otherwise record `unknown`. No separate USD ceiling was stated.
- Use the existing CodeGG `opencode_go` path, a recorded H007 manifest revision, and the exact unchanged H007 verifier. No alternate route or hidden-verifier access.

Authorization ledger: `plans/closure/harness-lab-self-improvement-resumption/evidence/s016-live-authorization-20261031.json`.

## 3. Frozen qualification contract

Runs 1–4 used `dev/harness_lab/fixtures/agent_autonomy_h007/cases.jsonl` (SHA-256 `c703c1cb256f268500029cf0ca35faf9a8f246485afb2e3e5d98f166f15e4358`). Diagnostics showed two task-calibration omissions: the prompt did not tell the model that qualification requires at least 30 minutes of substantive work, and the required corpus job completed in under eight seconds, before the model's next productive source edit. Runs 5–9 used the versioned corrective prompt at `dev/harness_lab/fixtures/agent_autonomy_s016_promptfix_v1/cases.jsonl` (SHA-256 `3bb3bcb6f802fcf317f064101bf116ca3b96111fa8d26984753a157350bb1df2`). Run 10 used prompt v2 at `dev/harness_lab/fixtures/agent_autonomy_s016_promptfix_v2/cases.jsonl` (SHA-256 `a23b374f06c06e1f3318f7d4033b2856430288d034bd0f41f034deb9be3eed93`). Run 12 onward uses prompt v3 at `dev/harness_lab/fixtures/agent_autonomy_s016_promptfix_v3/cases.jsonl` (SHA-256 `8aa4d4af9fc36334819a9e494d5f7f02d58437be9bed6fa1ad672b7d16f4f875`). V3 maps every asynchronous `test` call to exact-ID terminal polling, separates the one fake-ID boundary probe from real test IDs, explicitly retains all four safe probes, and reduces the pre-implementation subagent work to one concise read-only task. It leaves the five-crate Rust fixture, contract, corpus, public tests, hidden verifier, and qualification thresholds unchanged. The hidden verifier SHA-256 remains `13617671ceedfe1b4cff89c9a96ad5380fd94ca4799f861f95a404f84090d33c`.

A qualifying run must pass the independent hidden verifier and show, at minimum:

- at least 30 minutes of substantive active work with no measured activity gap above five minutes;
- both distinct seeded failure/repair/check cycles;
- exact-ID background-job submission, an observed `running` state during productive downstream edits, and observed terminal state;
- continuation evidence and all five dependent Rust phases complete;
- complete ACP observation with no dropped or unpaired tool events; and
- no stream idle timeout or other harness terminal failure that invalidates the measurement.

Task correctness, provider stream health, and horizon status remain separate in each receipt. A fast task completion is not a long-horizon pass. A task/verifier failure alone is not a CodeGG defect.

## 4. Scope and invariants

In scope: allowlist the single newly authorized H007 route; offline test the route policy; run the sequential long/diagnostic loop; and make evidence-supported local corrections. Prompt calibration may make existing qualification requirements explicit, improve the ordering of already-required work, and add the user's requested safe tool-boundary probes. It must not change the verifier, acceptance criteria, source contract, public tests, or corpus.

Out of scope: any other model/adapter, raw prompt or SSE logging, changing H007 acceptance thresholds, weakening the verifier, broad daemon/scheduler/ACP/job architecture changes without a reproduction, automated candidate promotion, or treating historical S013/S015 authorizations as transferable.

The S015 correction is the starting baseline: decoded-event idle timeout is 120 seconds. Preserve it unless a new diagnostic directly falsifies that behavior.

## 5. Ordered work packages

### A. Prepare and validate route

- Admit exactly `opencode_go/deepseek-v4.1-flash` in the H007 runner while keeping other routes refused.
- Add a provider-free route allowlist regression.
- Validate the corrected ARM64 binary and sibling sandbox helper, fixture identity, request-cap wiring, disk floor, and private lane paths before provider admission.
- Commit the controller, plan, and ledger before run 1.

### B. Run long qualification first

Run the current recorded H007 prompt revision against the unchanged five-crate source fixture, contract, corpus, public tests, and hidden verifier, using the exact route, a 60-request/3,600-second policy, and 10 GiB disk floor. Preserve the full receipt, causal trace, safe provider lifecycle log, admission receipt, binary and manifest identities, task/verifier results, and disk minima.

### C. Diagnose, correct, and repeat

If a long run fails, record the failure class from evidence. Start a separate DeepSeek diagnostic run only when it can distinguish model/task behavior from a CodeGG boundary. If a current CodeGG defect is reproduced, write a failing-first regression, implement the narrow correction, run focused verification, commit it, then retry qualification. If no CodeGG defect is reproduced but task instructions or background-work duration prevent measuring an existing criterion, preserve the evidence, make a versioned prompt calibration without changing acceptance, and retry. Runs 1–4 established that provider streaming, ACP event pairing, job execution, and durable job status work; the model submitted the job but ended the turn without a terminal poll, while the short job completed before a source edit. The prompt correction is the current corrective. All long and diagnostic runs consume the same 20-run ceiling.

### Run 5 finding (2026-10-31)

The prompt-calibrated long run used manifest SHA-256 `3bb3bcb6f802fcf317f064101bf116ca3b96111fa8d26984753a157350bb1df2` and exhausted all 60 provider requests after 6.88 minutes. Provider streams reached EOF and all ACP tool starts had terminals; there were no event gaps, stream idle timeouts, or disk-floor events. The model used two read-only subagents but made five job submissions without polling any exact returned ID; the only job status call was the failed fake-ID boundary probe. It did not write the probe report or continuation.

The same live log now provides direct CodeGG defect evidence: repeated `InvalidHistoryOrBudget` errors say compaction output failed tool-call history validation, followed by refusal of provider request 61 at the enforced cap. Run 6 is a focused exact-ID job-surface diagnostic. A failing-first ARM64 unit regression reproduced the compatibility `DropMiddleMessages` defect: the first/last-message reducer split a valid assistant/tool-call pair and returned `InvalidHistoryOrBudget`. The correction routes this compatibility reduction through the existing pair-safe emergency compactor. The focused regression and all 42 `context::compaction::tests` pass under the ARM64 Rust 1.89 toolchain; formatting and the test assertion/duplication guards pass. The unchanged H007 qualification prompt, fixture, verifier, and thresholds are next to be retried. Run 5 did not qualify.

### Run 7 diagnostic (2026-10-31)

The focused live diagnostic submitted one `catalog-format` test job and polled its exact returned ID (`e1034616-8c5b-42ff-8d06-dc8cdbdd508f`). CodeGG returned a terminal `failed` state for that job. All three ACP tool calls were paired and observed; the run used five provider requests and did not hit resource limits. The fixture's seeded test fails in this no-edit diagnostic, so its hidden verifier did not pass. This shows exact-ID status routing works for a terminal job; it does not reproduce the run-5 compaction failure. The S016 follow-up will isolate context compaction with a failing-first local regression before making any production change.

### Run 8 outcome after compaction corrective (2026-10-31)

Run 8 used the corrected ARM64 binary from commit `94fc8d4` with the unchanged S016 promptfix manifest, H007 fixture, verifier, and thresholds. It ended after 8.53 minutes at the 60-request cap. All 60 provider bodies reached EOF, ACP tool starts and terminals paired, no stream idle timeout or activity-stall fired, and disk stayed above 10 GiB. The run log contained no invalid-history compaction error, but it also did not log entry into the `DropMiddleMessages` compatibility path; live traversal of the corrected branch remains unproven.

The agent submitted five test jobs and recorded a continuation file with the last returned ID, but it never queried any returned exact ID. Its only `job status` call was the failed fake-ID probe. It modified frozen README/contract inputs, omitted the probe report and helper, and ended with zero repair cycles. The harness-run verifier failed in `catalog-query` (0 tests run). Run 7 had shown exact-ID status works on a simple prompt, so run 9 will use a concise lifecycle-first diagnostic that requires one exact test-job ID, continuation write, and status-to-terminal before additional work. After that diagnostic, simplify the long prompt's test/job sequencing without changing H007 acceptance criteria, then retry.

### Run 9 diagnostic (2026-10-31)

Run 9 used the H007 frozen inputs and diagnostic-v2 manifest. The read-only subagent was spawned and observed `queued` then `completed` through the task surface. The agent submitted the specified corpus test and received exact job ID `1162a582-fb1a-45ee-a89e-1f0a7449af9b`, wrote a continuation file, then made three exact-ID status calls and one exact-ID output call. Every observation reported terminal `failed`; no running state or productive overlap was observed, and the horizon verifier did not accept the continuation. All 17 provider requests reached body EOF; the 11 ACP tool start/terminal pairs were complete with zero event gaps, no idle timeout fired, and disk remained above 25 GiB. This run does not reproduce a provider-stream, ACP, task-ID, or job-ID-routing defect. It shows the diagnostic test command terminates failed before there is an overlapping work interval. A continuation tool call alone is insufficient evidence unless its contents satisfy the horizon recorder. Prompt v2 makes the `task`/`job` ID boundary and continuation payload explicit; the H007 fixture, verifier, and acceptance criteria remain unchanged.

### Run 10 outcome after prompt v2 (2026-10-31)

Run 10 stopped at the 60-request cap after 9.16 minutes. Provider and ACP telemetry remained healthy: 59 of 60 bodies reached EOF, no idle timeout or invalid-history/compaction error was logged, all 49 tool starts had terminals, and there were zero event gaps. Disk remained above 17 GiB. The agent made three `test` submissions (IDs `9ff61c3d-c79f-49a1-9011-fd09b4ded732`, `e419a2a6-4de4-48b9-93c1-e588c5d5ddd7`, and `7bd9ef10-d4d1-4bb7-a363-c8398465cbcc`) but made no exact-ID `job` status call. Its sole `job` call was the fake-ID boundary probe. `fixture/CONTINUATION.md` contained `job_id: pending`; the horizon evidence recorded zero repair cycles, no continuation, and no exact job lifecycle. One source file changed in the isolated lane, but the independent verifier did not run because the turn ended at the provider cap. This does not reproduce a stream, ACP, or durable-job projection defect. Run 9 already demonstrated that a focused DeepSeek task can use `test` → exact returned ID → `job status` and observe a terminal state. The next diagnostic isolates ID retention across repair-test submissions and the corpus job, then prompt v3 will make each test job's terminal observation and the final overlap sequence a single ordered checklist. The H007 source inputs, verifier, and acceptance criteria remain fixed.

The follow-up diagnostic is recorded at `dev/harness_lab/fixtures/agent_autonomy_s016_job_sequence_diagnostic_v3/cases.jsonl`. It requires three sequential `test` submissions, a terminal status for each exact returned job ID, and a final continuation containing the third ID; it forbids source edits and unrelated tools. It is intended to distinguish ID-tracking/task-sequence behavior in a short live task. If it reproduces correct handling, the remaining corrective is to reduce prompt complexity and limit pre-implementation subagent/tool overhead while keeping all qualification requirements.

### Run 11 job-sequence diagnostic (2026-10-31)

Run 11 submitted the `catalog-format`, `catalog-store`, and `catalog-index` corpus tests sequentially. It used each returned exact ID with `job status` and received terminal `failed` for all three (the first was polled twice). The continuation immediately after the third submission contained the exact corpus ID and recorded the first two terminal states; the final status was then observed as `failed`. All 10 provider bodies reached EOF, all 8 ACP tool calls paired, no event gaps or idle timeout occurred, and disk stayed above 18 GiB. This confirms that DeepSeek can retain and poll multiple sequential test-job IDs in the focused task; it does not reproduce a CodeGG job or transport defect, and its seeded tests did not pass or exercise running overlap. Prompt v3 applies this exact sequencing to each repair/check and the priority overlap job, while reducing one source of pre-implementation request overhead. The next step is a fresh long qualification on the same H007 fixture and hidden verifier.

### Run 12 long qualification (2026-10-31)

Run 12 used prompt v3 and stopped after 60 requests in 613.849 seconds. All 60 requests received response headers and body EOF; there were no decoded-event idle timeouts, no ACP gaps or unpaired tool calls, and no dropped causal events. Request 61 was denied before transport by the configured cap. The agent completed two repair cycles, wrote continuation evidence, and observed terminal states for five exact test-job IDs, but none was observed running and no productive edit occurred while a job was active. Its active span was 596.001 seconds, with a maximum activity gap of 66.103 seconds, far short of the 30-minute requirement. The hidden verifier did not run because the capped agent turn ended inconclusively. Disk stayed above 10 GiB (minimum 14.50 GB; end 26.77 GB); reported cost was $0.049266. This does not reproduce a streaming, ACP, or job-status fault. It does show the current implementation/test sequence uses the request budget before the required horizon. Run index 13 was a launcher preflight refusal caused by a missing budget file; no provider request was admitted and it does not count against the 20-live-run limit. Run 14 then diagnostically attempted a natural offline release workspace test, exact-ID continuation, and dependent edits. No artificial delay or H007 acceptance change is permitted.

### Run 14 natural-overlap diagnostic (2026-10-31)

Run 14 used the offline release workspace-test diagnostic, with 20 provider requests, a 900-second cap, and no artificial delay. After 18 requests spent on inspection and edits, the model submitted two test jobs despite the one-job instruction. It wrote no continuation and made no `job status` calls for either returned ID (`0e7291bd-617e-4a5a-89c6-46d324766114`, `4fd8cb96-ade1-4e5e-8c56-a9152e0c54da`). The run ended at the request cap after 92.471 seconds. All 20 provider bodies reached EOF; tool events paired with no gaps, idle timeout, or disk-floor event. The harness automatically invoked the unchanged verifier, which ran zero tests; its contents were not inspected. This diagnostic did not reach the concurrency observation and does not reproduce a CodeGG execution defect. Run 15 narrows the diagnostic further: its first action is the single test submission, followed immediately by an exact-ID continuation, two contract-based edits, and status polling of that one ID.

### Run 15 first-action overlap diagnostic (2026-10-31)

Run 15 made the full-workspace release test its first tool call and wrote a continuation next. The harness returned job ID `fb9267c1-5335-4087-bfde-ed316b62f4b2`. The agent then made six reads and five shell calls, with no source edits or status poll, before the 14-request cap ended the turn after 95.253 seconds. Continuation evidence did not meet the horizon recorder. All 14 provider bodies reached EOF, all 14 ACP tool calls paired, no idle timeout/event gap occurred, and disk stayed above 30 GiB. The unchanged harness verifier was automatically invoked and ran zero tests. This still does not reproduce a job execution/status fault; it demonstrates that repeated inspection and shell activity displace the instructed status/edit sequence under a small diagnostic budget. For the next long run, prompt v4 will batch the mandated probes, keep the implementation phases explicit, and start a release workspace job after index work so dependent query/CLI development can overlap with a natural build. H007 inputs and acceptance thresholds remain fixed.

## 6. Required evidence and verification

- Append-only run authorization ledger; each started run records start/terminal time, route/model, source and binary digests, requests, cap status, USD or unknown, disk start/minimum/end, and outcome.
- Per-run sanitized receipt and references/hashes for private log, provider-admission receipt, causal trace, and verifier output.
- Preserve failed attempts. Do not rewrite prior H007, S013, or S015 records.
- Run focused Python tests for the H007 route gate and each corrective; run `scripts/verify.sh quick` when repository disk headroom permits. Any Rust build/run must use a private ARM64 target and continuously enforce the 10 GiB floor.
- After the first completed live run, run its unchanged independent verifier and inspect diagnostics before deciding whether a separate diagnostic run is useful.

## 7. Stop and completion conditions

Stop immediately on any authorization/resource/safety cap, unobservable request admission, sensitive-data exposure, source/binary identity mismatch, or operator cancellation. Stop without a fix if the failure is attributable to task/model behavior or the provider route and no CodeGG defect is reproduced.

Close positively only when one H007 run using a recorded manifest revision satisfies every frozen qualification criterion. Do not revise a manifest during a live run. Otherwise close at 20 started runs with an exact attribution summary and residual corrective owner; do not claim the harness is fully capable. S015's 16 unused diagnostic slots are not transferred into this authorization.

## 8. Initial operator handoff

The available ARM64 CodeGG binary is `/private/tmp/s016-bin-26ce84f/codegg` (SHA-256 `50a9ae92b7aac46f9fd9855612caa7b8fe98a505979fe23fd4578e55262d3ada`), with matching sibling helper SHA-256 `0f8db353fad3bf44ca9353aca6eab2162d933c4b8501748b05965bf7ff49f8b8`. Its Rust target cache was cleaned after preserving and verifying this pair. Recheck disk immediately before each run; free space recovered to about 39.9 GB after the owned cache cleanup. Rebuild only if a source correction requires it.
