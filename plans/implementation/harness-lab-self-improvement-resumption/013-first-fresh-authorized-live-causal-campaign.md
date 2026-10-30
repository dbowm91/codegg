# Harness Lab Self-Improvement S013 — First Freshly Authorized Causal Improvement Campaign

Status: **blocked (operational)** until S010/S011/S012 close, one independently reproduced **current** CodeGG defect is eligible, and new explicit campaign/provider authorization is obtained. Primary class: **capability**. Hard dependencies: S010 integrated native qualification, S011 real-defect intake authority, S012 causal evidence contract. Operational dependencies: live proposer route/credential/endpoint qualification and one new operator-approved budget. S003 historical `no_reproducible_defect` closure remains unchanged.

Authority: `plans/000-long-term-specification.md` §§23–24, 26–29; `plans/001-terminology-and-domain-model.md`; `plans/003-planning-process.md`; accepted `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md`; `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md`. Existing S001–S009 closure records are historical evidence, not re-opened milestones.

Baseline reviewed: `main@2b3785bfd743325f08149e9b6eb41828e823e4df` (2026-10-30), containing PR #219 and S001–S009 closure artifacts. At implementation, re-read `origin/main`, its CI, existing user changes and current `dev/harness_lab` interfaces; use the actual resulting source SHA in every receipt. Do not assume this planning baseline remains HEAD.

Ownership: development-only Python `dev/harness_lab` controls evaluation, qualification, admission and receipts. Native CodeGG daemon/broker remains authority for tools, permissions, jobs and ACP; `opencode_go` is the sole CodeGG provider adapter in scope; OpenCode CLI is an external proposer and never the evaluator.

## 1. Objective and existing evidence

Execute **one** bounded actual OpenCode proposal against one preregistered genuine current CodeGG harness defect and let the existing controller independently build, evaluate, hold out and accept/reject it. C003 showed one historical success; S007 newly enforces proposer pretransport N+1 admission against loopback; S010 completes composed offline pipeline; S011 supplies trustworthy real-defect eligibility. S013 is not permission to run a model now, and does not assume a positive fix will be found.

## 2. Preconditions (stop before launch on any failure)

- An S011 dossier is `defect_eligible=true` for a **current reproducible native CodeGG defect** with two independent red observations, exact source/binary/verifier/holdout and protected controls. Refuse H007 task failures, historical C003, synthetic seeded faults or provider-only errors.
- Freeze before proposal: source SHA, lane binary digest, case and suite digests, independent expected verifier, reserved holdout, negative controls, allowed product source prefixes, exact number/repeats of baseline and candidate tasks, success and retry policy, candidate cleanup/retention, resource ceiling, model route(s), budget and cancellation. No adaptive change to pass counts or holdouts after results.
- Obtain **fresh explicit per-campaign operator authorization** covering separate `opencode-go/provider-model` proposer route and `opencode_go/provider-model` CodeGG subject route, endpoint/account, pretransport request ceilings for each, wallclock/iteration/retry limits, disk floor, permission profile, cancellation, retention, cost information and known `USD=unknown` limitation. Never reuse H007 or S003 authorization. A subscription allowance is not billed USD.
- Verify actual installed OpenCode CLI version and live-capable gateway configuration against the frozen endpoint, including plugin/ambient proxy prohibition, credential isolation, redirected/refused request handling and current cap N/N+1 behavior. First use local loopback preflight without charged upstream; a subsequent **live** trial is disallowed until authorization. No implicit multi-provider adapter work.

## 3. Ordered work packages

A. **Freeze a current-defect experiment manifest.** Content-address and review S011 dossier, baseline source and independent test, protected cases, exact route/budget, intended score objective and all denominators; reject if rechecking source or verifier differs. Ensure candidate cannot edit proposer gateway, evaluator, tests or hidden holdout.

B. **Operational admission proof.** Preflight native CodeGG binary+sandbox helper, two isolated subjects, disk and path limits, explicit Deny and workspace-write/full-host policy acceptance as authorized, OpenCode gateway N+1 count, no ambient proxy, and cumulative request/budget accounting across interruption. Use only one CodeGG `opencode_go` adapter; proposer and subject remain distinct upstream request principals.

C. **Execute precisely the approved campaign.** Run frozen baseline then `optimize` with real OpenCode proposer in its private worktree and separately built native CodeGG candidate/ACP subjects; enforce hard limits before transport, live inactivity/interrupt exits, no unexpected permissions, no selective reruns, exact independent holdout, and immutable score contract. Preserve all rejected, accepted, incomplete, timeout and setup outcomes.

D. **Prove causal correction or truthful non-improvement.** An accepted candidate must pass the original red test now green, independent hidden verifier and protected controls, all exact baseline/candidate pairs, and a score with a genuine correctness improvement. Require evidence that the observed correction addresses the pre-registered failure mechanism, not a prompt, route, test suppression, terminal relabel or shortened horizon. A failed/rejected/no-defect/inconclusive run is a valid documented conclusion and does not license endless reruns.

E. **Handoff without promotion.** Generate a review bundle with defect identity, original red/green tests, exact patch and binary hashes, all result receipts, provider request admission counts, missing/billed cost classification, causal trace, resource/cleanup evidence, residual risks and proposed owning maintainer. The patch remains in the candidate worktree, never automatically merged/pushed. Cleanup must not destroy operator-owned artifacts.

## 4. Safety, failure, storage and migration

All new data remain versioned local sanitized receipts and existing ledger/checkpoint formats. No product protocol or DB migrations; any CodeGG product fix is scoped to the demonstrated defect and normal review process. SIGKILL/ACP loss, unreadable provider response, ambiguous admission ledger, orphaned daemon or unknown terminal must produce typed `inconclusive/blocked` status, retain cumulative budget, preserve owned artifacts for inspection and never replay an ambiguous provider request. Never mask uncontrolled authorization via `--auto` or bypass explicit Deny.

## 5. Verification

Before live execution, run S010–S012 offline regressions and gateway CLI/loopback N+1 preflight; after a real candidate, run appropriate frozen failing-first red/green test, fmt/build/focused tests, native ACP/holdout controls, Python suite, static guards, quick/full verification and exact-head hosted CI on any proposed source changes. Archive code/candidate and measurement SHAs separately; record missing tests as incomplete rather than pass.

```bash
python3 -m unittest discover -s dev/harness_lab/tests -v
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
git diff --check
scripts/verify.sh quick
# Run scripts/verify.sh full and exact-source-head hosted CI when production/integration code changes.
```

## 6. Acceptance, stop and closure

Close S013 as an **observed campaign** only with accurate terminal outcome, complete admission/identity/verification receipts and operator packet; a positive `accepted_for_review` is optional and may not be invented. If no eligible defect or authorization exists, keep **blocked**, not `closed`. If a campaign ran and failed, retain `rejected`/`no_improvement`/`inconclusive` with explanation. Do not claim `stable_live_evidenced` from one attempt. Stop on mismatched pinned inputs, deficient holdout, budget/cost-policy refusal, unauthorized path, sensitive output leakage, uncontained process, unknown provider admission or product defect outside scope. Closure: `plans/closure/harness-lab-self-improvement-resumption/013-status.md`, with real or blocked outcome, authorization provenance, tested candidate and explicit non-claims.
