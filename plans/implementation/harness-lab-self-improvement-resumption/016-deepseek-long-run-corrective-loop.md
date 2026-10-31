# Harness Lab Self-Improvement S016 — DeepSeek Long-Run Corrective Loop

Status: **active**

Repository baseline: `300cb4aba` (S015 timeout correction live-confirmed; S015 conditionally closed)

Source roadmap: `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#10-s016-deepseek-long-run-corrective-loop`
Long-term requirements: `plans/000-long-term-specification.md` §§23–29; `plans/001-terminology-and-domain-model.md`
Applicable ADRs: `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md`
Primary class: capability

## 1. Objective

Run the unchanged H007 five-crate Rust workload on `opencode_go/deepseek-v4.1-flash` to determine whether CodeGG can sustain a real long-horizon agent task after S015. If the long attempt fails, use a separate bounded live diagnostic run to identify whether the failure is a reproducible CodeGG harness defect, task/model behavior, or an unavailable provider route. Correct only a reproduced narrow CodeGG defect, verify it, and repeat the unchanged long run. Stop when H007's independent qualification passes or 20 total long/diagnostic runs have started.

## 2. Authorization and limits

The user explicitly authorized up to 20 runs on 2026-10-31 and specified DeepSeek 4.1 Flash. The route is `opencode_go/deepseek-v4.1-flash`. This authorization covers the sequence of long and diagnostic runs in this plan; it does not authorize another model or provider adapter.

- Maximum 20 started live runs total; long and diagnostic runs both count.
- Maximum 60 provider requests and 3,600 seconds per run.
- Keep at least 10 GiB free disk; cancel immediately if the floor is crossed or disk state becomes unreadable.
- Run sequentially. Cancel on operator request, cap exhaustion, sensitive-data exposure, or loss of provider-request admission observability.
- Record provider-reported USD when available; otherwise record `unknown`. No separate USD ceiling was stated.
- Use the existing CodeGG `opencode_go` path and the exact H007 manifest/verifier. No alternate route or hidden-verifier access.

Authorization ledger: `plans/closure/harness-lab-self-improvement-resumption/evidence/s016-live-authorization-20261031.json`.

## 3. Frozen qualification contract

Use `dev/harness_lab/fixtures/agent_autonomy_h007/cases.jsonl` unchanged (SHA-256 `c703c1cb256f268500029cf0ca35faf9a8f246485afb2e3e5d98f166f15e4358`) and the existing isolated fixture/verifier pair. A qualifying run must pass the independent hidden verifier and show, at minimum:

- at least 30 minutes of substantive active work with no measured activity gap above five minutes;
- both distinct seeded failure/repair/check cycles;
- exact-ID background-job submission, an observed `running` state during productive downstream edits, and observed terminal state;
- continuation evidence and all five dependent Rust phases complete;
- complete ACP observation with no dropped or unpaired tool events; and
- no stream idle timeout or other harness terminal failure that invalidates the measurement.

Task correctness, provider stream health, and horizon status remain separate in each receipt. A fast task completion is not a long-horizon pass. A task/verifier failure alone is not a CodeGG defect.

## 4. Scope and invariants

In scope: allowlist the single newly authorized H007 route without changing the fixture, verifier, caps, request-admission boundary, timeout correction, or provider integration; offline test the route policy; run the sequential long/diagnostic loop; and make evidence-supported local corrections only.

Out of scope: any other model/adapter, raw prompt or SSE logging, changing H007 acceptance thresholds, weakening the verifier, broad daemon/scheduler/ACP/job architecture changes without a reproduction, automated candidate promotion, or treating historical S013/S015 authorizations as transferable.

The S015 correction is the starting baseline: decoded-event idle timeout is 120 seconds. Preserve it unless a new diagnostic directly falsifies that behavior.

## 5. Ordered work packages

### A. Prepare and validate route

- Admit exactly `opencode_go/deepseek-v4.1-flash` in the H007 runner while keeping other routes refused.
- Add a provider-free route allowlist regression.
- Validate the corrected ARM64 binary and sibling sandbox helper, fixture identity, request-cap wiring, disk floor, and private lane paths before provider admission.
- Commit the controller, plan, and ledger before run 1.

### B. Run long qualification first

Run the unchanged H007 case with the exact route, a 60-request/3,600-second policy, and 10 GiB disk floor. Preserve the full receipt, causal trace, safe provider lifecycle log, admission receipt, binary and manifest identities, task/verifier results, and disk minima.

### C. Diagnose, correct, and repeat

If a long run fails, record the failure class from evidence. Start a separate DeepSeek diagnostic run only when it can distinguish model/task behavior from a CodeGG boundary. If a current CodeGG defect is reproduced, write a failing-first regression, implement the narrow correction, run focused verification, commit it, then retry the unchanged long qualification. If no CodeGG defect is reproduced, retain that evidence and continue the authorized loop with the same task/model and criteria; do not invent a harness fix. All long and diagnostic runs consume the same 20-run ceiling.

## 6. Required evidence and verification

- Append-only run authorization ledger; each started run records start/terminal time, route/model, source and binary digests, requests, cap status, USD or unknown, disk start/minimum/end, and outcome.
- Per-run sanitized receipt and references/hashes for private log, provider-admission receipt, causal trace, and verifier output.
- Preserve failed attempts. Do not rewrite prior H007, S013, or S015 records.
- Run focused Python tests for the H007 route gate and each corrective; run `scripts/verify.sh quick` when repository disk headroom permits. Any Rust build/run must use a private ARM64 target and continuously enforce the 10 GiB floor.
- After the first completed live run, run its unchanged independent verifier and inspect diagnostics before deciding whether a separate diagnostic run is useful.

## 7. Stop and completion conditions

Stop immediately on any authorization/resource/safety cap, unobservable request admission, sensitive-data exposure, source/binary identity mismatch, or operator cancellation. Stop without a fix if the failure is attributable to task/model behavior or the provider route and no CodeGG defect is reproduced.

Close positively only when one unchanged H007 run satisfies every frozen qualification criterion. Otherwise close at 20 started runs with an exact attribution summary and residual corrective owner; do not claim the harness is fully capable. S015's 16 unused diagnostic slots are not transferred into this authorization.

## 8. Initial operator handoff

The currently available corrected ARM64 CodeGG binary is `/private/tmp/s015-target-arm64/aarch64-apple-darwin/debug/codegg` (SHA-256 `7d06fc68058e1963a7a81dee54a581c9bce01063b17a90bd62940b9857a99f92`), with its matching `codegg-sandbox-helper`. The most recent observed free disk was 16,932,916 KiB. Recheck immediately before each run. Do not build the Rust workspace unless necessary; current run can reuse this exact S015-qualified binary.
