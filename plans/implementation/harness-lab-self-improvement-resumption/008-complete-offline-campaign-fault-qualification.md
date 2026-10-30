# Harness Lab Self-Improvement S008 — Complete Offline Campaign Fault Qualification

Status: **closed conditionally — offline fault matrix requalified; `stable_offline` not established**

Repository baseline: `e8dbb8af3793acf46741eae89b3c5509fc952e47` (PR #219)

Source roadmap: `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md`, S008.
Long-term requirements: `plans/000-long-term-specification.md` §§23–24, 26–29; `plans/003-planning-process.md`.
Applicable ADRs: `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md`.
Primary class: **invariant**.

## 1. Objective

Complete S005's missing campaign-level fault qualification using deterministic candidates, local HTTP fixtures and private CodeGG subjects. The result must establish stable offline behavior or retain a precise failing blocker; it must not claim real-model improvement repeatability.

## 2. Why this corrective is ready

S005's current closure explicitly leaves daemon-crash cleanup, sustained resource contention, and exhaustive post-admission/proposer/build/verifier/publication/cleanup transition injection unproven. S007 owns the external HTTP request gate. S008 may test it with fixtures and must not make live calls.

## 3. Current implementation evidence

`dev/harness_lab/improve.py` has hash-linked optimization checkpoints, cumulative budget restore and ledger validation. Existing tests inject selected SIGKILL boundaries, candidate build retries, evaluator failures, disk-floor cancellation and private subject cleanup. They do not inject each campaign transition and do not prove cleanup/reconciliation after the controller process itself is killed while private subjects exist.

## 4. Invariants that must not regress

- A crash cannot reset request, iteration, retry or wall-clock budgets, duplicate a proposal/admission, or change the score denominator.
- Every state transition resolves to one typed terminal or a durable `interrupted_unknown`; uncertain work cannot be retried automatically.
- Daemon/child cleanup is ownership-checked, idempotent and isolated from the user's default daemon or unrelated processes.
- Resource saturation and disk-floor failure cancel before the next expensive side effect and retain receipts.
- Deterministic candidate subjects cannot alter their evaluator, holdouts, campaign ledger or controller.

## 5. Scope

### In scope

- A table-driven fault matrix for every persisted campaign transition and external side-effect boundary.
- Controller kill/restart with owned child processes or private daemons present, followed by safe reconciliation and cleanup.
- Deterministic CPU/Cargo-target/disk pressure tests with isolated targets, bounded resources and recovery.
- Integration of S007's fixture-backed proposer N+1 evidence into the S005 matrix.

### Explicitly out of scope

- Live model/provider calls, production scheduler/daemon redesign, broad filesystem cleanup, or changing S006's live success threshold.
- Claiming `stable_offline` from unit tests alone when the actual optimizer-to-subject path or an owned cleanup boundary is untested.

## 6. Required production changes

Change only `dev/harness_lab` or test infrastructure where a red test proves the missing recovery contract. Add a narrowly scoped owned-process receipt/reconciliation mechanism only if current code cannot identify and safely clean its private children after parent loss. Resource tests must use private `CARGO_TARGET_DIR`, explicit concurrency caps and disk checks; no unbounded stress loop.

## 7. Ordered work packages

### A — Enumerate campaign fault transitions

Derive transition and side-effect boundaries from the actual `run_optimizer` / `qualify_candidate` flow. Include admission, proposer request, patch capture, build, independent verification, ACP evaluation, ledger append, score publication, checkpoint pointer update, cancellation and cleanup.

Acceptance evidence: matrix rows map each boundary to a named injected test, expected terminal, duplicate-effect assertion and cleanup assertion; no unlisted production transition remains.

### B — Prove parent-death cleanup

Run deterministic owned child processes/private subject daemons, kill the controller at named persisted points, resume/reconcile, and verify only exact owned resources are stopped or removed. Confirm default daemon identity and unrelated sentinel processes survive.

Acceptance evidence: repeatable crash/restart receipt, no orphan, no duplicate row/admission, exact original deadline and no cross-subject teardown.

### C — Qualify bounded contention and recovery

Use deterministic competing builds/jobs with per-subject Cargo targets and capped workers. Inject resource/disk pressure and cancellation at admission/build/verification boundaries; verify fail-closed stop and subsequent clean retry only where retry is explicitly authorized by the controller.

Acceptance evidence: fixed run count, peak/free-disk observations, active process inventory, typed terminals, and successful clean recovery without changing candidate evidence.

### D — Integrate request gate and close S005

Consume S007 local HTTP N/N+1 receipts. Repeat the complete matrix twice from reset isolated roots and compare normalized outcomes, identities and score denominators.

Acceptance evidence: deterministic replay matches; zero provider requests; all mandatory matrix rows pass or are named blockers. Mark `stable_offline` only if every criterion in S005/S006 is met.

## 8. Failure, cancellation, restart, and contention semantics

The first interrupted attempt remains charged and recorded. Recovery validates source, suite, binary, route, budget, target and owner identities before acting. Unknown process ownership, incomplete journals, disk read errors or stale deadlines stop fail-closed and preserve artifacts for inspection. Never terminate the user's daemon or clean an unowned path.

## 9. Compatibility and migration

Additive test hooks and receipts only. Existing checkpoint/ledger versions remain readable. Any durable schema or production process ownership change needs a separate review and must retain legacy refusal behavior.

## 10. Required tests

- Table-driven crash injection for every campaign transition and persisted append/pointer boundary.
- Parent SIGKILL with subject children, resume, exact-owned cleanup and unrelated-process survival.
- CPU/build-target contention, disk-floor breach, unreadable resource state, cancellation race, and no-resource-leak recovery.
- Gateway N+1 test from S007 integrated with campaign budget state.
- Repeat full deterministic sequence twice; compare receipt identities and unchanged denominators.

## 11. Required verification commands

```bash
python3 -m unittest dev.harness_lab.tests.test_improve dev.harness_lab.tests.test_supervisor dev.harness_lab.tests.test_budget
python3 -m unittest discover -s dev/harness_lab/tests -v
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
git diff --check
scripts/verify.sh quick
```

## 12. Documentation updates

Update S005 fault matrix and closure, Harness Lab operator guidance, and the roadmap/registry status. Separate component pass results from campaign-level stability.

## 13. Acceptance criteria

All source-derived transitions have non-vacuous injected coverage; parent-death recovery has no orphan or unowned teardown; bounded contention and disk failures stop safely; complete deterministic replays preserve request/admission counts, identities, denominator and terminal dispositions. If a named condition remains unproved, keep `stable_offline` not established.

## 14. Stop conditions

Stop on an unowned or unidentifiable process, any evidence of touching the user's default daemon, an unbounded resource test, a needed production scheduler change, or a recovery contract that cannot preserve the consumed-budget/denominator rules. Do not simulate a live provider result.

## 15. Closure evidence required

Provide the transition inventory, fault-to-test map, repeated receipt digests, process/resource/disk inventories, cleanup and negative-control evidence, S007 gateway evidence, exact local and hosted verification, and final stable-offline verdict.

## 16. Handoff notes

Run serially when touching process-global env or Cargo targets. Keep all scratch roots under a task-specific temporary directory and inspect disk before and after stress cases.
