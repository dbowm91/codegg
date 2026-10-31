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

Runs 1–4 used `dev/harness_lab/fixtures/agent_autonomy_h007/cases.jsonl` (SHA-256 `c703c1cb256f268500029cf0ca35faf9a8f246485afb2e3e5d98f166f15e4358`). Diagnostics showed two task-calibration omissions: the prompt did not tell the model that qualification requires at least 30 minutes of substantive work, and the required corpus job completed in under eight seconds, before the model's next productive source edit. Run 5 onward uses the versioned corrective prompt at `dev/harness_lab/fixtures/agent_autonomy_s016_promptfix_v1/cases.jsonl` (SHA-256 `3bb3bcb6f802fcf317f064101bf116ca3b96111fa8d26984753a157350bb1df2`). The revision makes the existing horizon explicit, directs immediate continuation/status observation and preplanned source edits around the job, and adds the user's requested read-only subagents, small helper, and safe boundary probes. The five-crate Rust fixture, contract, corpus, public tests, hidden verifier, and qualification thresholds remain unchanged. The hidden verifier SHA-256 remains `13617671ceedfe1b4cff89c9a96ad5380fd94ca4799f861f95a404f84090d33c`.

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
