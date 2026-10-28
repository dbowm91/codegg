# A005 Live Placement Comparison — 2026-10-28

Status: **complete; no-go**

Implementation plan: `plans/implementation/agent-worktree-placement-and-build-affinity/005-conservative-placement-advisor.md`
Closure: `plans/closure/agent-worktree-placement-and-build-affinity/005-status.md`
Machine-readable redacted receipts: `plans/validation/agent-worktree-placement-and-build-affinity/005-live-placement-comparison-20261028.json`

## Scope and method

The paired comparison covered six cases: dependency-heavy Rust inspection, Rust isolated edit/build, Python read-only inspection, Python isolated debug, a Spanish isolated edit, and refusal for an unavailable exact managed checkout. Each case had three randomized baseline/advisor pairs. Both arms used the same binary, scheduler, tool authority, and approved provider route. The baseline received the generic typed placement contract; the candidate received the conservative placement advisor. Hidden verifiers were isolated from each agent workspace.

The first run used the original six-case manifest. It reached the original 240-request total ceiling after 25 task trials, leaving 11 frozen schedule slots refused. The remaining authorization then increased to a cumulative 480 requests and 120 minutes while retaining the $10 Go-allowance ceiling. The 11 refused slots were resumed from a redacted, hash-checked checkpoint. A review then found that the unavailable-checkout case had not been classified as a protected negative control. Its task intent and hidden verifier already required exact-target refusal. The manifest now declares that same case protected, with a typed `unavailable_managed_checkout` trigger, and its six paired rows were rerun. The amended control classification changes scoring only; the prompt and verifier are unchanged. Both manifest hashes and the amendment are disclosed in the JSON artifact.

## Budget and execution

The approved cumulative ceilings were **480 provider requests, 120 minutes, and $10 reported OpenCode Go allowance**. Across the initial interrupted attempt, corrected full schedule, and supplemental control repetitions, the run used **384 requests**, **$0.182919 reported allowance**, and **2,724.928 seconds** conservatively accounted. The request and allowance caps remained enforced at the provider boundary; only one inference was in flight. OpenCode reports this allowance after a response completes; it is not a cash-charge figure.

All 36 scheduled task rows received a terminal receipt. The six supplemental control rows were also completed. Per-trial provider admissions and allowance receipts, duration, CPU, RSS, disk growth, placement requests, and verifier results are recorded in the redacted JSON. Raw provider response bodies, workspace paths, transcript IDs, and exact managed-worktree identities are excluded.

## Results

The baseline and candidate each had **0/18 independently verified task successes**. The protected unavailable-checkout control held **0/3** times in both arms. The shared scoring contract returned `rejected` with objective `none`: correctness pass rate was 0.0 in both arms; reliability was 0.889 baseline and 0.944 advisor; median end-to-end duration was 39,724.5 ms baseline and 37,715 ms advisor. Reliability and duration improved, but both are advisory under this contract and cannot accept a candidate without a substantive correctness improvement.

The outcome is **no-go**. These observations do not support a stable placement strategy that improves successful work, and the exact unavailable-checkout control failed in both arms. No correctness, scheduler-wait, collision-reduction, or performance benefit is claimed. ACP does not expose scheduler queue wait or exclusive-lease collision outcomes.

## Disposition

A005 is closed no-go. Keep the deterministic guidance within its existing authority boundary; do not promote it as model-qualified or add automatic placement, cache, shared-target, or shared-checkout behavior. Future qualification requires a new plan after the delegated task failures and negative-control behavior are addressed.
