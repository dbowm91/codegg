# Harness Lab Self-Improvement S003 — Bounded Causal Self-Improvement Campaign

Status: **closed as `no_reproducible_defect`; zero live calls**. No eligible current CodeGG defect exists in the frozen H007 evidence. See `plans/closure/harness-lab-self-improvement-resumption/003-status.md`.

Repository baseline: `main@99cecc050ed1ae418af62d2d10e6d34d791513e1` (PR #217 merged); re-pin actual main and controller implementation at handoff.
Source roadmap: `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md`, S003.
Long-term: `plans/000-long-term-specification.md` §§23–24, 26–29; `plans/001-terminology-and-domain-model.md`; `plans/003-planning-process.md`. Accepted ADR: `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md`. Primary class: **capability**.

Predecessor records: `plans/closure/harness-lab/005-status.md`, `plans/closure/harness-lab-post-pilot-corrective/003-status.md` (one accepted live correction), `plans/closure/harness-lab-continuation/001-status.md`, H006/H007 closures, and new S001/S002 closures.

## 1. Objective

Demonstrate a **repeatable, bounded, independent assess → propose → build → score → accept/reject self-improvement campaign** on a real, pre-registered CodeGG agent-facing harness defect, using the already implemented Python controller and isolated candidate worktrees. The campaign may conclude `accepted`, `rejected`, `inconclusive` or `no_reproducible_defect`; no positive improvement is presumed or fabricated. Establish *measured causality* and no regression, not model ranking or a compulsory 30-minute endurance score.

## 2. Hard and operational prerequisites

**Hard:** S001 diagnosis closed; S002 current-main optimizer, controlled replay, negative-control acceptance and restart gates strictly closed; PR #217 integrated, with resulting-mainline CI confirmed by S002 closure; a specific baseline test/hidden holdout is frozen with a legitimate reproducible CodeGG failure (for an *accepted improvement* claim only). If no real defect exists, the campaign may prove no-defect refusal using no live calls, but it cannot claim a real accepted improvement.

**Operational:** before *each* provider-backed attempt the operator explicitly approves (a) CodeGG subject route `opencode_go/<model>`, (b) optional OpenCode CLI proposer route and controlled credentials, (c) exact provider request caps for each actor, (d) cumulative wall-clock and process cancellation limits, (e) disk free floor and Cargo contention limits, (f) dollar spend cap if genuinely enforceable or cash cost `unknown`, (g) iteration/retry ceiling, and (h) stop behavior for unknown request accounting and provider timeouts. Historic H004–H007 authorizations are consumed; this document is not one. The external OpenCode CLI proposer cannot borrow the native CodeGG subject's request cap.

## 3. Existing capabilities and evidence limits

The existing `dev/harness_lab/improve.py` optimizer can create scoped worktrees, call an optional OpenCode proposer, compile and test candidates, evaluate separate ACP subjects, compare immutable rows and stop on substantive correctness improvement; its M005/C003 receipts establish one real accepted and one real rejected candidate. It is not yet evidenced as a repeatably successful, current-main campaign after H007.

H007 offers a challenging Rust fixture and seven negative live samples, including two typed provider timeouts, incomplete repairs and no duration-qualified run; these alone do **not** identify a CodeGG regression to patch. Do not make the model fix H007's *task implementation* and call that CodeGG self-improvement. A candidate must change the harness behavior (tool selection/contract, event outcome, context recovery, job usability, provider admission, etc.) under a reproducible test attributable to CodeGG, not lower verifier expectations, change a prompt after scoring, or increase test duration.

## 4. Invariants/non-goals

- Controller, verifier, holdout and fixtures are externally frozen and inaccessible to candidate/proposer. All candidate patches are scoped to named agent-harness source paths. No automatic merge/push; human accepts the result for code review.
- Version every experiment: baseline SHA/hash, candidate patch hash, compiled binary digest, fixture/holdout digests, route/account profile identifier (never secret), policy, daemon identity, tool/event sufficiency and budget receipt.
- A *correctness_fix* acceptance is pre-registered before proposal. A faster but incorrect result, terminal relabel, missing verifier, changed fixture or synthetic historical seed **cannot** count. Compare identical case IDs, repeats, cohort, policy, provider route and immutable input digests. Do not pool across provider routes or models as if one treatment.
- Preservation of negative controls and no introduced regression is mandatory. Report test-level outcomes with explicit denominators; missing events/proxy timeout are not agent failures and cannot be opportunistically excluded. Any inability to score keeps outcome inconclusive.
- Candidate autonomy cannot alter its own harness/evaluator/ledger, controller budget, permissions or live policy. No second provider adapter, uncontrolled endpoint failover, offline fake fixture rebranded live, training, or changed native tool broker authority.

## 5. Scope

In: one independently reproduced failing agent-harness behavior and one predeclared protected+holdout suite; a bounded multi-iteration real proposer campaign or explicit no-defect refusal; paired baseline/candidate ACP trials and independent scoring; native regression and scope checks; live safety/budget admission; comparison with no-proposer/manual control if useful; operator-ready reviewer receipt.

Out: automatic production rollout/merge/push, broad code optimization outside CodeGG's agent-facing harness, requiring multiple native adapters, claiming statistical model superiority without sample power, changing H007 frozen verifier, spending without fresh approval, mandatory 30-minute observation for normal correctness improvements.

## 6. Required changes and compatibility

**Prefer running existing code unchanged.** A new developer-only campaign manifest/reporting adapter is acceptable where needed for preregistration, repeat identities and evidence provenance. Fix source only when S002 or an S003 failing-first proof establishes a concrete defect in the owning CodeGG or controller subsystem. No new daemon feature or schema by default.

**Experiment manifest** (new versioned local file or existing manifest with additive fields): immutable baseline SHA, policy, model route and proposal route, target defect statement, red test and hidden pass criteria, protected negative controls, candidate allow-prefix set, paired repetition design, bound budgets, stop predicates, and independence/measurement assumptions. The manifest is frozen *before* authenticated proposal. Candidate cannot modify this or its digest.

**Comparison/reporting:** separate provider timeout, request-cap stop, incomplete measurement, ordinary task verifier failure, non-reproduced regression, task correctness gain and optional horizon result. Report the number of scheduled, completed, excluded/unmeasured and paired cases; never silently replace failed attempts with extra trials to force acceptance. Require a changed independently verified correctness predicate with zero protected regressions. Report latency/resource changes as secondary. Where randomized model trials vary, show per-run variance/uncertainty and refrain from global reliability claims from a handful of samples.

**Security/permissions:** use a named `yolo` profile only on private development lanes; `workspace-write` for containment-sensitive comparisons and `full-host` only by explicit operator selection. Enforce scope against symlink traversal; retain native Deny, audit, broker and sandbox checks. Runtime cleanup and redacted evidence must work even on a proposer or provider failure.

## 7. Ordered work packages

**A — Preregister the genuine defect.** Consume S001's reproducible candidate queue, inspect current main, run the baseline failing-first test under native CodeGG ACP, and write a frozen short causal hypothesis with two plausible alternative explanations and disconfirming observations. Name `tool_contract`, `ACP_event`, `context_recovery`, `job_observation`, etc. only if supported. Freeze at least one independent holdout that is not leaked to proposer. If none is reproducible, stop with `no_reproducible_defect` and zero provider calls; document remaining candidate search separately.

**B — Build the bounded measurement design.** Establish before running: identical suite + per-arm repeated trial IDs, fixed policy and route, max iterations and repairs, cache/Cargo isolation, independent verifier hash, comparability denominator and cancellation criteria. Include historical C003 negative-control class (cosmetic patch, incomplete patch, altered expected outcome) as explicit rejection controls. Decide sample size and acceptance threshold in advance based on expected variance and budget; if only a small sample is approved, limit claims to that experiment. Prefer at least two independent observed post-patch trials for any stochastic claim; correctness regression test can be deterministic. No mixing pre- and post-hoc scoring rules.

**C — Fresh authorization gate.** Show a credential-free preflight receipt with explicit per-actor cap enforcement (CodeGG subject AND OpenCode proposer), approved route(s), run duration, disk floor, account/rate uncertainty policy and process-kill path. Ask for operator approval; do **not** auto-run when approval is missing. Missing hard request admission on proposer/subject or unreliable usage receipt is a stop, not a soft warning. Keep real billed USD unknown when the provider exposes only allowance/quota units.

**D — Execute and score at most the approved campaign.** Invoke existing `optimize` against the frozen baseline, proposer and worktree; label each candidate/retry, record objective outcome and repeat evidence, then run independent held-out qualification. Reject any regression, incomparable pair, missing source identity or escaped patch. Stop early on a correctly accepted candidate or an allowed exhaustion terminal; preserve failed candidates instead of deleting evidence. No post-hoc rerun after a failed provider/measurement outcome without a specifically preregistered bounded retry rule and remaining budget.

**E — Deliver and disposition.** Produce a compact causal comparison of baseline vs candidate, negative controls, failure classes, exact receipts, observed resource impacts, remaining uncertainties and patch/diff reference. If accepted, retain the candidate branch/worktree for manual review but do not merge, deploy or push it automatically. If rejected, report which independent criterion failed. If inconclusive/no defect, identify the next *diagnostic* experiment rather than declare success.

## 8. Failure, cancellation, restart, contention

An interrupted experiment may resume only with identical immutable experiment identity and original cumulative deadline; already scored trial IDs and consumed requests are not replayed. The controller is the authority for process kill and cleanup; private daemon states are stopped on cancellation and initial allocation failure is non-destructive. A provider timeout is typed and cannot be converted into candidate correctness failure; if request accounting or key state is uncertain, refuse further calls. Concurrent candidates cannot share writeable fixtures, mutable data, worktrees, Cargo targets or daemon identity. Disk-floor breach terminates without candidate acceptance.

## 9. Compatibility/migration

No new CodeGG protocol; use ACP v1. Previous M/C/D/H score receipts remain historical. New campaign schema must read older records as explicitly `unknown` where a field was never observed. C003's accepted improvement remains one historical event, not data to pad current sample size. No default product config or permission change, no automatic remote Git mutation. Preserve independent H007 task/horizon scoring.

## 10. Required tests

- Credential-free campaign dry run demonstrates frozen preregistration, allowed patch scope, independent verifier and holdout, true accepted-vs-rejected score and no-op/advisory/incomplete candidate rejection.
- Live provider admission gate tested locally against fake endpoints: request N admitted, N+1 refused before transport, timeout not retried across lost accounting; independent proposer cap cannot be bypassed by resetting subject counter.
- Process fault injection after proposal, build, one scored pair and before final score: resume no duplicate scored run and preserve original deadline/caps.
- Security: candidate attempts to write controller, hidden verifier, corpus or neighbor checkout are refused; secrets/prompt bodies absent from receipts; native explicit Deny remains effective.
- Paired comparison: incompatible route, policy, case digest, baseline revision, verifier digest or incomplete numerator/denominator is unscorable; a faster correct task with short horizon stays correct.
- Actual live run(s), **only after approval**, must include red baseline reproduction, candidate scope/build verification, protected controls, independent held-out score, complete stop and cleanup receipts. A none/rejected campaign closes honestly without claiming accepted improvement.

## 11. Verification commands

```bash
python3 -m unittest discover -s dev/harness_lab/tests -v
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
git diff --check
scripts/verify.sh quick
# If product implementation changes, run relevant cargo tests, full verification,
# and exact-source-head hosted CI before proposing candidate acceptance.
# Operator-approved live provider/proposer commands are intentionally absent:
# derive them from the frozen campaign manifest and fresh explicit authorization.
```

A trial's exit status alone is never proof of correctness, tool observation completeness, or billed spend. Include actual commands/run IDs only in the closure record.

## 12. Documentation/guards

Extend `dev/harness_lab/README.md` with one safe operator campaign example, environmental preflights, distinct proposer/subject admission, observable stop conditions, report interpretation and manual candidate handoff. Add a non-vacuous manifest freeze/digest guard and cost-source test if existing guards do not cover them. Update registry only after evidence, not when a model suggests success.

## 13. Acceptance criteria

One bounded campaign's every provider admission, candidate, test pair, score, refusal, cleanup and budget is auditable. The campaign returns a truthful terminal: `accepted` with a real protected, repeatable correctness improvement on an independent verifier, `rejected` with explicit failed gates, `inconclusive` with named missing evidence, or `no_reproducible_defect` with no live provider use. Successful scoring is **not** a mandate to automatically merge a patch. No model-ranking or 30-minute sustained-autonomy claim is inferred.

## 14. Stop conditions

Stop for no genuinely reproducible harness defect (unless reporting no-defect outcome), missing S002 closure, no new explicit authorization, inability to enforce a separate proposer hard cap, ambiguous provider usage, unverifiable holdout isolation, loss of exact policy/daemon identity, sustained measurement event gaps, unsafe disk floor, scope escape or required broad architecture revisions. A changed recovery termination policy requires its own ADR/owner; do not silently revise Option B.

## 15. Closure evidence

`plans/closure/harness-lab-self-improvement-resumption/003-status.md` must identify operator approvals and consumed amounts, pinned manifest/suite/holdout/control hashes, real baseline defect/red test, proposer and candidate patches, per-arm comparable trial IDs, hard admission receipts, reviewer-signable positive and negative score evidence, SIGKILL/restart when used, actual CI/guards and known severities. A no-live/no-defect closure must say so explicitly and cannot be described as a successful AI-driven repair.

## 16. Handoff

Keep the campaign small enough for one developer-workstation session and one candidate at a time unless the operator explicitly budgets independent subject lanes. A stronger `opencode_go` upstream route may be tested in a separate preregistered campaign; **do not** build a second CodeGG adapter or claim provider diversity as a blocker to this initiative. Remain on the existing controller, evaluator and immutable ledger wherever possible.
