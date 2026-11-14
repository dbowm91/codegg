# Harness Lab Self-Improvement Resumption S063 — Final Muse Requalification

Status: **active; one final authorized attempt 84**

Repository baseline: `4af27640c6661768685fa3e4315d3bd7009d97d4`

Source roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#58-s063--final-muse-requalification`

Long-term requirements:

- `plans/000-long-term-specification.md` §§23–24, 26–29
- `plans/003-planning-process.md` §§6–7

Primary class: infrastructure

## 1. Objective

Use the one remaining start in grant `s019-supplemental-20261111-04` for attempt 84 against the unchanged v97 H007 task, verifier, manifest, route, and acceptance criteria. Reuse the exact ARM64 release pair built from Rust source commit `4af27640c6661768685fa3e4315d3bd7009d97d4`; repository changes for S063 are documentation/evidence only. Determine whether the attempt 83 `job.status` correction enables a complete, valid task closeout under the same conditions.

Do not change the task prompt, fixture, verifier, qualification predicates, CodeGG runtime, scheduler, daemon, ACP, request policy, or resource caps. Attempt 83 met the active-work horizon and all implementation/job/review gates but timed out with invalid continuation section/pointer ordering. The remaining uncertainty is whether the model can produce a valid closeout on the same frozen contract after the status-query semantic correction.

## 2. Attempt 83 evidence

Attempt 83 exercised the correction: 49 status calls were projected; the failed job states were successful observations, and the only failed status lookup was a correlated `job_not_found`. It reached 3,295.240 active seconds, completed all seven stages, five contract phases, twelve gates, two repair cycles, review, release overlap, and final workspace-test/Clippy jobs. Event pairing and applicable job-ID correlations were complete.

The task did not qualify because its continuation progress sections and pointer fields were out of order. The agent hit the prompt-work timeout, leaving an incomplete transcript terminal; the verifier failed `continuation_boundary` and `transcript_job_lifecycle`. S062 closes the specific status-query defect. No further runtime defect was attributed.

## 3. Invariants and bounds

- Use only `opencode_go/muse-spark-1.3-contributor`.
- Preserve v97 task, verifier, manifest, and qualification gates exactly.
- Use the ARM64 pair with main binary SHA-256 `ff25d37aab1abcc577a1d3c1e23c5b80d835637270947fdd58f175bb090739d9` and helper SHA-256 `1336a576407742ef9e519b37e284e7c582b2b8c50098550dde2793fc304835ef`.
- Before provider admission, require a clean side-effect-free preflight, exact manifest and binary hashes, available route, valid ledger start 84, one grant start remaining, and at least 30 GiB free disk.
- Enforce 1,200 provider requests, 1,600 native calls, 3,300 seconds prompt work, 3,590 seconds external wall, 300 seconds native-tool inactivity, and the 10-GiB runtime disk floor. External cancellation uses a 15-second graceful termination period before force termination.
- Poll the provider-boundary safe request aggregate during execution; do not use the unreconciled campaign ledger as a live request counter. Do not inspect private run materials. USD is unknown if unreported.

## 4. Acceptance and stop conditions

Attempt 84 qualifies only if the unchanged verifier passes, the public continuation is valid and ordered, the transcript reaches completed, all task stages and gates pass, exact-ID background-job lifecycle and productive overlap pass, both repair cycles pass, active work reaches at least 30 minutes with no activity gap over 300 seconds, and all request/tool/disk/event bounds hold.

Stop before provider admission if any preflight condition fails, if the route or exact binary pair is unavailable, or if the grant is exhausted. Stop the campaign after attempt 84 whether it passes or fails. If it remains unqualified, record the model/task closeout result and do not start another live run without fresh explicit authorization; do not invent another harness corrective absent new evidence.

## 5. Closure evidence

Record source/build identity, all focused checks already passed for S062, fresh preflight and safe run evidence hashes, live and reconciled request counts, duration and active-work span, event and job-ID correlation, exact repair/lifecycle results, continuation ordering and terminal state, verifier result, disk minimum/end, stop reason, authorization counters, and unknown USD when unreported. Never include private receipt/log/transcript/tool data or credentials.
