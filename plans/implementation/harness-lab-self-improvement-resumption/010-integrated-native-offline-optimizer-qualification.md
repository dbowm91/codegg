# Harness Lab Self-Improvement S010 — Integrated Native Offline Optimizer Qualification

Status: **closed offline**. Primary class: **invariant**. Closure: `plans/closure/harness-lab-self-improvement-resumption/010-status.md`. Both paired native optimizer campaigns, prolonged resource pressure, and the real native ACP trial SIGKILL/restart gate passed. S011's trusted intake/refusal gates are also complete, establishing `stable_offline`; no live improvement claim is made.

Authority: `plans/000-long-term-specification.md` §§23–24, 26–29; `plans/001-terminology-and-domain-model.md`; `plans/003-planning-process.md`; accepted `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md`; `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md`. Existing S001–S009 closure records are historical evidence, not re-opened milestones.

Baseline reviewed: `main@2b3785bfd743325f08149e9b6eb41828e823e4df` (2026-10-30), containing PR #219 and S001–S009 closure artifacts. At implementation, re-read `origin/main`, its CI, existing user changes and current `dev/harness_lab` interfaces; use the actual resulting source SHA in every receipt. Do not assume this planning baseline remains HEAD.

Ownership: development-only Python `dev/harness_lab` controls evaluation, qualification, admission and receipts. Native CodeGG daemon/broker remains authority for tools, permissions, jobs and ACP; `opencode_go` is the sole CodeGG provider adapter in scope; OpenCode CLI is an external proposer and never the evaluator.

## 1. Objective and evidence gap

Close precisely the condition left open in `plans/closure/harness-lab-self-improvement-resumption/005-status.md`, `008-status.md` and S006: the real `run_optimizer` must run a **whole paired native CodeGG subject campaign** (actual candidate build, private daemon/ACP, real verifier, comparison and protected controls), rather than a synthetic candidate-qualification callback. Repeat deterministically under bounded, separately targeted Cargo contention and deliberate crash/restart. This is an offline equipment/measurement qualification, **not** an accepted current-CodeGG fix.

Existing evidence: S002 proved native two-daemon isolation and tool routing; S005/S008 proved six durable phase SIGKILLs, N+1 refusal and bounded parallel Cargo while the optimizer used a **synthetic qualification callback**; H006/H007 proved native job observation separately. The missing composition cannot be inferred from these individually passing tests. C003's historical authenticated fix is not a new current campaign.

## 2. Non-goals and invariants

- No provider calls, login, billing, remote OpenCode proposer, new scheduler, VM, multi-provider support, GUI or daemon protocol redesign. Never imply the synthetic red regression is a newly discovered real defect.
- Candidate may change only scoped product source and its required changelog; evaluator, scoring contract, suite, verifier, hidden holdout, baseline and budgets remain outside candidate scope and immutable.
- Use a private disposable Git clone/worktree and an **explicitly labelled synthetic/seeded regression** against a pinned checkout: do not commit a breakage to `main` or change permanent project tests. The no-op and actual scoped correction must be differentiable by independent native behavior.
- Two independent subject daemon identities, private roots, sandbox helpers and distinct `CARGO_TARGET_DIR`s; never use the user's default CodeGG daemon. A parent-death test must not kill unrelated daemons.
- Every decision consumes complete, exactly paired `task_id`/repeat keys, unchanged case digests, exact source/binary fingerprints, truthful negative controls and terminal/error distinctions. Correctness is the only default acceptance-carrying criterion.

## 3. Expected changes and bounded interface

Modify `dev/harness_lab/tests/test_improve.py` (and narrow fixtures/helpers) to drive **the existing `run_optimizer`**, not a parallel workflow. If current direct `opencode_propose` dependency prevents a deterministic offline campaign, add a **test-only injectable proposal function/adapter**, with production default unchanged and no externally selectable fake-proposer CLI mode. The offline proposer deposits a reproducible allowlisted patch for a pinned, deliberately regressed private source state; the candidate proceeds through unmocked `qualify_candidate`, `_run_gate`, `evaluate`, native ACP and `scoring`. Do **not** mock compilation, ACP, verifier, baseline/candidate comparison or score publication.

Choose a minimal native tool-path defect whose red and green outcomes are independently exercised through a real CodeGG binary (a private historical headless-binding replay may be used only if it works unchanged on current main). If a deterministic mutation is needed, document its injection point, hash, authorization-free test-only provenance and independent oracle. Freeze verifier, suite, controls and acceptance conditions before running either arm. Require an explicit negative/no-op candidate and a positive candidate; their score outcomes must diverge for **real** native reasons, not injected scores.

## 4. Ordered work packages

A. **Pin and calibrate the regression.** Establish `red` from a native base binary and `green` from a separately built corrected binary; prove all protected controls and the independent verifier discriminate; prove the unmodified current `main` is not thereby classified defective. Record baseline SHA, seeded mutation SHA, fixture/suite/holdout digests and expected terminals.

B. **Compose the actual optimizer.** Build the frozen baseline ledger with real ACP; feed one no-op candidate and then one correcting candidate through one `run_optimizer` execution, its checkpoint/event log, candidate worktrees, native build and ACP verification, and scoring. Ensure the candidate cannot write into the measuring controller/holdout. Do not bypass `qualify_candidate` with an injected result.

C. **Repeat and recover.** Run the entire campaign twice from newly created private roots; assert identical normalized decision and frozen identities (timestamps/PIDs may differ). Inject SIGKILL at a durable phase on a *real* native trial; resume once, or refuse correctly if the outcome is ambiguous, without duplicate proposal, trial row or provider request. Preserve cumulative deadline and resource caps across restart.

D. **Native contention and cleanup.** During an integrated campaign, run two separate bounded one-worker Cargo checks on disjoint target roots for two rounds, recording CPU/RSS/free disk if available and `unknown` otherwise. This is **not** prolonged saturation; make one explicit prolonged resource-pressure exercise with a predeclared finite horizon, no sleeps as fake agent activity, a disk floor and bounded controller concurrency. If infeasible on the workstation, record a conditional blocker instead of claiming `stable_offline`. All daemons and process groups must be reaped; check the default daemon is unchanged.

E. **Publish machine-checkable evidence.** Version a sanitized `s010-integrated-offline.json` receipt with per-arm source/binary identity, real ACP run IDs, control verdicts, actual gate commands, fault point/recovery, Cargo targets and resource observations; no raw secrets, tool arguments or credentials.

## 5. Verification and falsifiers

Focused tests must demonstrate: missing `evaluate` invocation fails the test; a fake successful verifier cannot pass; a changed baseline case digest or candidate binary refuses; protected control regression rejects; an unmatched terminal and orphaned attempt cannot be accepted; a no-op candidate is rejected; an actual compiled candidate fixes the seeded native failure; crash/restart never duplicates the same pair; unrelated daemon survives. Re-run the two integrated cycles from fresh roots rather than merely calling pure score functions.

```bash
python3 -m unittest discover -s dev/harness_lab/tests -v
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
git diff --check
scripts/verify.sh quick
# Run scripts/verify.sh full and exact-source-head hosted CI when production/integration code changes.
```

## 6. Storage, compatibility and operations

Keep `dev/harness_lab` local and development-only. Any new receipt field must be additive and versioned; preserve existing S001–S009/H007 JSON bytes and score contract semantics. Existing `optimize` CLI and live admission behavior must not change. No new CodeGG persistent storage or public protocol. Cleanup only identified owned lanes/worktrees/targets; log failures as typed incompleteness, never blanket-delete operator files.

## 7. Acceptance, stops and closure

S010 closes strictly only after two complete integrated native campaigns, a rejected no-op and accepted synthetic correction, independently frozen controls, a real ACP crash/restart outcome, bounded repeated contention **and** declared prolonged resource-pressure evidence all pass with full cleanup. Otherwise close conditionally and list precise missing gates. S010 does **not by itself** assert `stable_offline`: real-defect intake positive/negative authority is S011. Stop on evaluator leakage, source/holdout modification, default daemon access, missing binary hash, uncontrolled resource use, or any live call. Closure at `plans/closure/harness-lab-self-improvement-resumption/010-status.md` must include source/commit/CI, requirement-to-evidence matrix, reproducible commands, paired receipt IDs, no-live-call assertion, security/contention findings, limitations and promotion recommendation.
