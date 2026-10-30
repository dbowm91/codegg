# Harness Lab Self-Improvement S006 — Real Campaign Repeatability and Stable Operator Handoff

Status: **closed conditionally — `stable_live_not_established`**. No eligible defect, fresh live authorization, or complete S005 campaign-level fault matrix is available. See `plans/closure/harness-lab-self-improvement-resumption/006-status.md`.

Baseline reviewed: `main@99cecc050ed1ae418af62d2d10e6d34d791513e1` (2026-10-30); re-pin integrated main at implementation.
Source roadmap: `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md`, S006.
Primary class: **capability**. Hard dependencies: S003 first causal campaign disposition, S004 reproducible intake, S005 offline fault qualification. Operational dependency: new per-campaign operator authorization for any live provider/proposer use.

Authority: `plans/000-long-term-specification.md` §§23–24, 26–29; `plans/001-terminology-and-domain-model.md`; `plans/003-planning-process.md`; accepted `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md`; all S001–S005 closure/evidence and original Harness Lab M/C/D/H records.

## 1. Objective

Establish the *limited, honest operational meaning* of "stable autonomous self-improvement": the existing controller can repeatedly accept independent, real CodeGG harness improvements **when independently proven**, reject spurious proposals and return meaningful no-defect/inconclusive outcomes without exceeding permission, budget, time, disk or experiment-identity bounds. Produce an operator-grade, reviewable, **manually promoted** improvement artifact and a repeatability report. Do not claim that agent/model performance or a 30-minute endurance threshold has been solved.

## 2. Current evidence and scope delta

Historical C003 contains one legitimate accepted fix and a rejected non-improving candidate. S003 plans one new bounded campaign, which might validly end without an accepted fix. Even a successful S003 campaign does not establish a stable repeatable service. S004 provides eligible defect dossiers; S005 hardens the complete controller against fault transitions. S006 measures the composed workflow using distinct, preregistered *current* defects and an operator workflow, not a new optimization engine.

No reliability percentages or expected acceptance frequency may be claimed from C003 plus one new heterogeneous task as though they were exchangeable independent samples.

## 3. Non-regression invariants and non-goals

- Every accepted result must improve a **frozen correctness predicate on real CodeGG agent-harness behavior** and pass independent holdout, security negative controls, regressions and comparable baseline/candidate trials. Candidate cannot change evaluator, controller, budget authority or frozen cases.
- Never claim improvement from Rust fixture completion, model reasoning quality, terminal relabeling, latency-only change, altered prompts, test suppression, token savings alone, missing events or selective reruns.
- Historical and current trials remain separate cohorts. Pre-register defect identity, case/route/policy, baseline SHA, verifier and holdout hashes, control group, comparison logic and repeat count before any proposal; report `n`, non-exercised and inconclusive cases.
- All subject requests use the single CodeGG `opencode_go` adapter; optional external OpenCode proposer has independent, proven pretransport admission. Fresh explicit approval per campaign, no inherited H007 or S003 budget. No keys or raw provider outputs in reports.
- No automatic Git merge/push, production daemon changes, unlimited iterative loop, default full-host opt-in, custom remote orchestration, required container/VM, multi-provider adapter parity, global performance rankings or forced 30-minute wait.
- Human controls continuation and promotion. An accepted patch is a **review candidate**, not an approved production change.

## 4. Intended interfaces and production changes

Prefer **no CodeGG production changes**. Use the existing S003 campaign manifest, S004 defect dossier, S005 qualification checks and `dev/harness_lab` `optimize`/ledger/reports. Add only a compact local operator command or runbook for `preflight -> freeze -> approve -> execute -> compare -> inspect -> archive/cleanup`, if current CLI cannot express it. Keep a read-only campaign index (content-addressed immutable receipt references), distinct statuses `offline_ready`, `live_authorized`, `accepted_for_review`, `rejected`, `inconclusive`, `no_reproducible_defect`, `live_blocked`, `not_run`. A local report is sufficient; no web dashboard or daemon service.

An accepted candidate retains a branch/worktree/diff and exact tested binary fingerprint for human review; it never pushes or merges itself. Cleanup of rejected candidate roots follows existing ownership validation and a documented retention window; preserved evidence is sanitized and immutable.

## 5. Ordered work packages

**A — Freeze the promotion standard and runbook.** Define in writing, before new experiments, that `stable_offline` requires S005's complete fault matrix and `stable_live_evidenced` requires repeated genuine independently verified improvements **on distinct preregistered CodeGG defects**, using S004's eligibility proof. Keep `stable_live_not_established` if sufficient genuine defects or authorization are unavailable. Record the original C003 accepted trial as historical context, not a fresh current-main replicate.

**B — Assemble a bounded, independently owned cohort.** Reuse S003's eligible live defect and seek one **different current** CodeGG harness defect supported by S004. Distinct means independently failing native/ACP behavior and disjoint or explicitly identified failure mechanisms, not the same bug under two prompts. If only one or zero exists, do not invent or seed a "real" defect; close with `repeatability_not_established` and a diagnostic queue. Frozen negative/no-defect case and protected controls must also be exercised offline.

**C — Operational authorization and admission.** For each real campaign, obtain new explicit operator approval for subject route, external proposer route if used, each separately enforceable request ceiling, wallclock, total iteration/repair ceiling, disk floor, cancellation, log retention, account/rate uncertainty and cash cost source. Test live-offline boundary with N+1 refusal and no accidental credential use in offline mode; if a proposer hard cap cannot be demonstrated, do not launch it. No spend budget is inferred from subscription allowance.

**D — Execute repeated independent campaigns.** Run bounded paired baseline/candidate ACP sessions, holdout/protected controls and predeclared repeat count for each eligible defect, one candidate at a time on private sources/daemons/targets. Preserve all proposals, failed builds, rejected patches, provider timeouts, incomplete traces and no-defect dispositions. **Do not retry for a favorable p-value or pass count**; use fixed retry policy established before the first run. Compare per-defect and overall denominators, with uncertainty and host/route/policy limitations. An accepted fix on a deterministic contract is not automatically evidence of improved stochastic model task success.

**E — Human review and stable-status evidence.** Generate reviewer packet: original failing test, patch diff, exact source/binary hashes, independent verifier/holdout and negative controls, paired results, usage/cost sources, daemon/candidate cleanup, open severity findings and explicit non-claims. Human accepts or rejects integration separately. Publish concise operator instructions including cancel, resume, `no_reproducible_defect`, `live_blocked` and evidence retention/cleanup. No automatic rollout.

## 6. Acceptance and stability labels

`stable_offline`: S004 eligible/negative dossier gates and S005 repeated fault/restart matrix pass on actual current main with no missing safety predicate; all result paths honestly terminal. This is offline operational readiness, **not** evidence of a stable real-model improvement rate.

`stable_live_evidenced`: at least **two newly executed independent, preregistered current CodeGG harness defect campaigns** yield accepted, independently proven correctness fixes with zero protected regressions, plus at least one correctly rejected or no-defect control; all mandatory identity, admission, event, cleanup and cost-unknown receipts complete. This is a small-sample capability demonstration, not a quantified reliability guarantee or a statistical model-ranking claim. Report individual and aggregate denominators, and state the small-n limitation.

When fewer than two independent real accepted campaigns exist, or no suitable defect is found, close S006 as **conditional/not established**, with an explicit reason and no fabricated positive result. Do not alter the denominator or substitute historical C003 for a new campaign to force closure. If a current real campaign fails, keep the result; diagnose it under the appropriate owner, not under acceptance criteria retrospectively.

## 7. Failure, restart, security and compatibility

No hidden retries on provider timeout or unknown accounting; no budget reset across crashes. A detached/orphaned process may not silently resume until identity and receipts reconcile. Proposer/subject permissions and provider admissions are distinct; worktree, Cargo target, daemon home, evaluator and protected controls remain isolated. A failed cleanup produces a blocked terminal with owned resource inventory and no broad system deletion. Candidate patches never change the controller, test corpus or stored outcome. Use the existing local ledger; only additive optional reporting schema, no CodeGG core storage or protocol migration. Cross-platform claims limited to actually tested workstation/OS.

## 8. Required tests and verification

- Native two-private-daemon/ACP end-to-end for the frozen eligible defect and negative control; no default daemon access.
- Candidate patch scope/holdout escape refusal; no-op, terminal relabel, skipped verifier, incomplete pair, shorter horizon but correct task, missing terminal, mixed route, changed binary hash.
- Proposer and subject N+1 pretransport refusal, timeout and unknown-usage stop, cumulative cap across resume.
- Deterministic `stable_offline` report with accurate denominators, no-defect and failed-campaign labels, clean retention/cleanup; at least two independent real campaigns **only with fresh authorizations**.
- Real campaign verification: appropriate Rust fmt/strict Clippy/focused+full tests, whole Harness Lab Python suite, assertion/duplication guards, exact-source-head hosted CI and a reviewer packet before any human-approved merge.

```bash
python3 -m unittest discover -s dev/harness_lab/tests -v
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
git diff --check
scripts/verify.sh quick
# scripts/verify.sh full and exact-head hosted CI when code/candidate integration changes
# Live commands are deliberately absent: derive from separately frozen authorization.
```

## 9. Stop conditions and closure

Stop on missing S003/S004/S005 strict closure, no independent real defect (report conditional), inability to enforce the external proposer's budget, candidate-accessible holdout, incomplete event integrity, uncontrolled permissions, unverifiable source/binary identity, request/cost accounting uncertainty, disk/cancellation breaches, or a required broad daemon/ACP/provider redesign. Escalate proven defects to their owning subsystem as separately scoped correctives.

Closure `plans/closure/harness-lab-self-improvement-resumption/006-status.md`: full requirement-to-evidence table, predeclared cohort and denominator, each independent campaign's immutable source/candidate/verifier/control/route identity, accepted/rejected/no-defect and interrupted outcomes, verified budgets/cleanup, CI/tests, review artifacts, operational guide, remaining risks and explicit `stable_offline` vs `stable_live_evidenced` verdict. **No automatic deployment, no success presumed.**
