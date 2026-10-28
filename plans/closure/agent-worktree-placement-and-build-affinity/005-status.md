# Agent Worktree Placement and Build Affinity A005 — Closure Status

Status: **closed (no-go)**

Source implementation plan: `plans/implementation/agent-worktree-placement-and-build-affinity/005-conservative-placement-advisor.md`
Source subsystem roadmap: `plans/subsystems/agent-worktree-placement-and-build-affinity-corrective-addendum.md#milestones-and-boundaries`
Live evaluation: `plans/validation/agent-worktree-placement-and-build-affinity/005-live-placement-comparison-20261028.md`
Machine-readable redacted receipts: `plans/validation/agent-worktree-placement-and-build-affinity/005-live-placement-comparison-20261028.json`

Closed: 2026-10-28

Primary class: capability / polish and live qualification

## 1. Executive finding

The deterministic placement advisor is implemented, but its required paired live-model qualification produced a **no-go**. Across six task cases, three randomized paired repetitions, and 36 scheduled task trials, neither the baseline nor the advisor arm received an independent hidden-verifier pass. The advisor improved reliability and median duration modestly, but the score contract treats those measures as advisory and requires a substantive correctness result. A post-evaluation review corrected the unavailable-checkout verifier and rechecked the six saved control transcripts: all six controls held. No positive task-correctness, scheduler-wait, collision-reduction, or performance claim is made.

The scoring contract returned `rejected` with objective `none`: correctness pass rate was 0.0 in both arms; reliability was 0.889 baseline and 0.944 advisor; median duration was 39,724.5 ms baseline and 37,715 ms advisor. After the offline control recheck, the protected control held 3/3 times in both arms. The score reports advisory reliability and efficiency improvements but no substantive task-correctness improvement. The decision is therefore **no-go** for treating the advisor as a qualified strategy or claiming a benefit.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Conservative typed placement guidance and rationale | Task schema, `docs/tools.md`, deterministic placement tests | pass | Advice does not grant paths, write authority, cache use, or quota expansion. |
| Production-scheduler placement and refusal contract | A001/A002 strict closure records and production scheduler tests | pass | See `001-status.md` and `002-status.md`. |
| Three paired repetitions across Rust, Python, multilingual, and unavailable-checkout cases | 36 scheduled task rows in the redacted A005 receipt | complete | All task rows have a terminal receipt; six additional control rows replaced the initially misclassified refusal rows for final scoring. |
| Independent task correctness | Hidden verifier results | no-go | 0/18 verifier passes in each arm. |
| Protected unavailable-checkout refusal | Six saved supplemental transcripts rechecked with corrected verifier | pass in both arms | 3/3 controls held in each arm; no provider calls were added. |
| Provider ceilings | Provider-boundary request and allowance receipts | pass | 384/480 requests; $0.182919/$10 reported allowance; 2,724.928/7,200 seconds conservatively accounted. |
| Cost and resource evidence | Per-trial provider, CPU, RSS, disk-growth, and duration receipts | recorded | OpenCode Go reports allowance, not cash charge. |
| Scheduler wait and exclusive-lease collision measurements | ACP projection | unavailable | No reduction claim is made for these outcomes. |

## 3. Qualification method and amendment

The comparison used `opencode_go/muse-spark-1.3-contributor`, seed `20261028`, and the same binary in both arms (`d8d1f63b792828c970fb5b5eeedbf63251715d9a10e6a4cc7e0c97d42242cc94`). The baseline received the generic typed placement contract; the candidate received the conservative advisor. The source revision, tools, scheduler authority, per-case verifiers, and randomized schedule were held constant.

The original six-case manifest mistakenly classified the unavailable exact managed checkout as an ordinary task, so the score contract could not measure its required negative-control criterion. Its task prompt and hidden verifier already required refusal without fallback. The fixture was amended to classify this unchanged case as a protected control and declare the `unavailable_managed_checkout` trigger. The six baseline/advisor control rows were then rerun under the remaining approved budgets and substituted before final scoring. The initial and amended manifest hashes and this scoring-only correction are recorded in the JSON receipt; no successful outcome was created by the amendment.

## 4. Budget accounting

The operator approved a cumulative cap of **480 provider requests, 120 minutes, and $10 of OpenCode Go reported allowance**. The request and allowance ceilings applied to all live attempts, including the first interrupted attempt, the resumed 36-row schedule, and the supplemental control repetitions. The final totals were **384 requests**, **$0.182919 allowance**, and a conservative **2,724.928 seconds**. One inference was in flight at a time. The $10 bound is against provider-reported allowance, not cash charged.

The request cap was not exhausted. The provider reported allowance after each response; the controller enforced the remaining ceiling before admitting further provider work. The durable receipts and cumulative calculation are in the machine-readable artifact.

## 5. Per-case outcome

The JSON artifact contains baseline and advisor results for every repetition, including requested placement, acceptance, task-run status, verifier outcome, request/allowance use, CPU, RSS, lane disk growth, and duration. Across the five non-control cases, every case had 0/3 verifier passes in each arm. The unavailable-checkout control held 3/3 in each arm after offline rechecking its saved transcripts with the corrected verifier. The Spanish edit case is included with its original task intent unchanged.

Task reliability rose from 16/18 (0.889) to 17/18 (0.944), and advisor-arm median duration fell by about 5.1%. These secondary movements do not establish a correctness benefit under the frozen acceptance contract. The language and placement samples are too small and have no successful task outcome from which to infer a stable placement rule.

## 6. Verification executed

- `python3 -m unittest discover -s dev/harness_lab/tests`: passed, 367 tests with one skipped.
- `scripts/verify.sh quick`: passed, including formatting, generated assets, all guards, Harness Lab tests, and `cargo check --workspace --all-targets --locked`.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed.
- `cargo test -p codegg-providers --lib allowance --locked`: passed, 4 tests.
- `git diff --check`: passed.

## 9. Unavailable-checkout control investigation

The original verifier's assertion was invalid in three ways: it required exactly one task-tool call, required a scheduler-accepted task to be synchronously marked failed by that same tool call, and treated CodeGG's `.codegg/` state and `codegg_debug.log` as fixture mutations. In the saved runs, the task tool sometimes returned a queued job and the later status query reported the delegated run failed; two model trajectories also retried after an initial immediate tool error. The verifier therefore rejected safe outcomes before evaluating the control contract.

The queued job is refused late, during `SubagentJobExecutor`'s exact-worktree resolution. For an unavailable ID, the executor marks the run failed and returns before constructing or dispatching the child agent. The executor does not substitute the parent root or allocate a worktree. The production-scheduler test covers the same missing-ID terminal failure. This is late rejection and retry behavior, not an authorization bypass. The corrected verifier accepts either immediate refusal or a queued run that later reaches failed status, requires the exact requested placement and no completed child run, tolerates only the known runtime artifacts, and still requires exactly one worktree.

The corrected verifier was run offline against all six saved control transcripts; all six passed. Two focused verifier tests also prove queued refusal with retries/runtime artifacts passes and a completed child fails. No provider requests were made during this correction. The remaining end-to-end bottleneck is substantive delegated-task correctness: 0/18 non-control task trials passed in either arm. The scheduler's late rejection remains a user-facing reliability limitation and should be considered in future scheduler admission work; no placement optimization or automatic-policy claim follows from this control result.

## 7. Invariant review

Placement advice remains a typed recommendation only. `auto` remains the default. `isolated_new` and exact existing-managed-read-only selection retain their production authorization and scheduler gates. No cache, shared target, shared-checkout write, or automatic fallback was added. A005's result does not authorize a broader placement policy.

## 8. Roadmap disposition

A005 is closed **no-go**. Retain the conservative deterministic guidance, but do not promote it as a model-qualified strategy or make a performance claim. Any future work to address the observed delegated-job failures or qualify placement improvements requires a new plan with corrected task execution, explicit negative controls, and an approved evaluation budget. No additional A005 implementation remains open.
