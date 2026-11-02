# Harness Lab Self-Improvement S014 — Independent Live Repeatability and Human Promotion Boundary

Status: **blocked** on S013 observed positive genuine-current-defect campaign, a **different** independently eligible current CodeGG defect from S011, offline stability evidence (S010/S011/S012), and fresh separate live authorization for a second campaign. Primary class: **capability**. S013 rejection/no-defect outcome does not satisfy the positive hard dependency; in that case keep S014 blocked and report the missing evidence instead of retroactively weakening it.

Authority: `plans/000-long-term-specification.md` §§23–24, 26–29; `plans/001-terminology-and-domain-model.md`; `plans/003-planning-process.md`; accepted `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md`; `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md`. Existing S001–S009 closure records are historical evidence, not re-opened milestones.

Baseline reviewed: `main@2b3785bfd743325f08149e9b6eb41828e823e4df` (2026-10-30), containing PR #219 and S001–S009 closure artifacts. At implementation, re-read `origin/main`, its CI, existing user changes and current `dev/harness_lab` interfaces; use the actual resulting source SHA in every receipt. Do not assume this planning baseline remains HEAD.

Ownership: development-only Python `dev/harness_lab` controls evaluation, qualification, admission and receipts. Native CodeGG daemon/broker remains authority for tools, permissions, jobs and ACP; `opencode_go` is the sole CodeGG provider adapter in scope; OpenCode CLI is an external proposer and never the evaluator.

## 1. Objective

Evaluate whether the stabilized Harness Lab autonomously corrects **two different current CodeGG harness defects** in independently preregistered, bounded, native, real-provider experiments and truthfully refuses a no-op or no-defect control. S006's `stable_live_not_established` conclusion (zero of two new accepted campaigns) remains historical. This plan creates no permission to rerun H007, reuse C003, seed fake production defects or autonomously merge.

The target label `stable_live_evidenced` means limited small-sample demonstrated capability, **not** an estimated general success rate, ongoing 24/7 reliability or long-horizon qualification.

## 2. Invariants and cohort definition

A qualifying campaign is one current independently reproducible CodeGG **product harness** defect, distinct at failure-mechanism level and independently held-out, with new operator approval, a complete frozen baseline/candidate comparison and an accepted correctness repair. Two prompts on one bug are one mechanism and do not form two independent demonstrations. Both fixes must have zero protected-control regression, exact source/case/route/policy identity, complete accounting and manual review packet. Include at least one **pre-registered** no-op/non-improving or genuinely no-defect control with correct rejection. Do not reuse historical C003, seeded synthetic S010/S011 or H007 seven attempts in the denominator.

Use the same scoring semantics across both campaigns or explicitly report version differences; never cherry-pick successes, change retry counts, or reclassify model/task failures as CodeGG defects. Report separate per-campaign outcomes, actual observation windows, uncertainty and missing data. If provider route or host differs, stratify rather than claiming exchangeable observations.

## 3. Ordered work packages

A. **Freeze independent cohort.** Revalidate S013 as positive (not merely a completed campaign), identify S011 dossier for a different real defect and freeze its native red observation, holdout, negative controls and trusted verifier. Independently review that the mechanisms are genuinely distinct. If no second defect exists, stop and preserve `stable_live_not_established`.

B. **Second campaign admission.** New human authorization for proposer and subject routes, max provider requests per principal and aggregate, wallclock, iterations, cost policy, disk floor, sandbox profile, cancellation and retention. Re-run the exact S007 pretransport gateway and native CodeGG subject preflight; prohibit inherited budget/caps and automatic retries after ambiguous requests.

C. **Execute and verify.** Use S013's same bounded `optimize`/native qualification instrument with separate frozen experiment ledger and candidate source roots. Validate exact failing-first behavior, independent green test, protected holdouts and paired correctness. Run and document the predeclared no-op/no-defect negative control. Collect causal trace and honest incomplete outcomes. No auto push/merge.

D. **Aggregate reliability honestly.** Produce a deterministic read-only `campaigns report` from immutable dossier/receipt references, reporting counts for eligible, attempted, accepted, rejected, no defect, inconclusive, authorization blocked, and cleanup failures. Define `stable_offline` from S010/S011 and S005/S008 evidence separately from `stable_live_evidenced`. Do not estimate a confidence interval as if two heterogeneous defect cases were independent draws from a well-defined population.

E. **Human review and operations.** For each proposed fix, provide source SHA, patch diff, tested binary digest, red/green native reproductions, holdout and control verdicts, receipt manifests and hashes, provider request accounting vs billed USD `unknown`, list of observed risk/deferrals, manual merge instructions, and retention/cleanup. Maintain a compact operator runbook for blocked, cancelled, no-defect, rejected and accepted states. Reviewer is free to reject either candidate despite an accepted qualification score.

## 4. Verification and expected code

Favor only dev-only reporting and tests; do not invent a second optimizer, long-running daemon or model-ranking service. Reuse S010–S013 contracts; implement deterministic aggregation if existing `report/compare` cannot represent distinct experiments. Unit tests must reject reusing the same defect under two IDs, historical/synthetic dossier promotion, absent protected controls, incomplete pair, altered freeze, duplicate receipts and acceptance manufactured by changing score version. Ensure follow-up status never counts S013 twice. Record hard provider budget refusal, cleanup and cancellation handling.

```bash
python3 -m unittest discover -s dev/harness_lab/tests -v
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
git diff --check
scripts/verify.sh quick
# Run scripts/verify.sh full and exact-source-head hosted CI when production/integration code changes.
```

## 5. Storage, security, compatibility and stop

Only additive local versioned immutable report references; no CodeGG core DB or ACP migration. Candidate remains candidate-owned; controller/holdout protected. Existing authorization and credential separation unchanged. If route credentials, approved limits, binary/source identity or trace completeness are unknowable, mark the corresponding run inconclusive/blocked. Never claim a provider USD charge from quota units.

## 6. Closure and promotion criteria

`stable_live_evidenced` requires **at least two new** independently accepted real current-defect campaigns, genuinely different mechanisms, one correctly rejected/no-defect control, complete admissible source/route/budget/holdout/capture/cleanup receipts and human-review artifacts. Any missing element means `stable_live_not_established`, even if the plan's bounded report tooling itself is closed. No full autonomy in source promotion: every merge/push is human-owned. Historical H007 30-minute duration remains a separate H-line qualification and is neither required nor counted here. Closure: `plans/closure/harness-lab-self-improvement-resumption/014-status.md`, containing cohort denominator, per-campaign evidence, exact-head CI, review decisions, residual failure modes and explicit limited stability label.
