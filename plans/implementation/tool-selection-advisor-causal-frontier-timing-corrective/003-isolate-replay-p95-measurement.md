# Tool-Selection Advisor Timing Corrective C003 — Isolate Replay p95 Measurement

Status: closed — replay p95 gate passed in the exclusive CI group

Repository baseline: `03e20d3edf5b4ee07f002f61084d5c88e808a273`

Source corrective: `plans/subsystems/tool-selection-advisor-causal-frontier-timing-corrective-addendum.md`
Controlling process: `plans/003-planning-process.md#7-corrective-passes`
Controlling architecture: ADR-0009, `architecture/agent-tool-surface.md`

## 1. Trigger and objective

Hosted `CI / verify` runs `38840695582` and `38842540498`, both on the same
blackboard branch, failed at `causal_observe_replay::replay_p95_within_budget`.
The measured p95 values were 5.559 ms and 5.597 ms against the frozen 5.0 ms
budget. Each failed after 6,300 or fewer tests and left more than 7,500 tests
unrun, so neither run qualified the blackboard trajectory.

The gate is a distributional performance check over 201 warm samples. Keep the
5.0 ms contract and the assertion unchanged. Run its test binary with the
existing CPU-exclusive nextest group used for other timing-sensitive suites,
so unrelated test processes do not compete with the sample window. This is a
same-workflow scheduling correction; it does not create a separate or optional
CI lane.

## 2. Invariants and scope

- Do not change `CAUSAL_OBSERVE_P95_BUDGET_MS`, the frozen records, or any
  measurement/assertion in production or test source.
- Do not skip the test or the remaining workspace suite.
- Do not increase Cargo build parallelism or nextest's global test slots.
- Keep the full workspace suite under the existing `ci` profile; only the
  `causal_observe_replay` binary joins the existing `threads-required =
  "num-cpus"` group.
- Preserve all prior C001/C002 closure records unchanged.

## 3. Work packages

A. Add `binary(causal_observe_replay)` to the existing CPU-exclusive nextest
   override. Keep the CI command and all frozen performance budgets unchanged.
B. Run formatting and config/diff checks; obtain hosted CI on the exact change
   head. Confirm the replay binary executes under the exclusive group, its p95
   assertion passes, and all workspace tests complete.
C. Record the exact run, test count, and any remaining failure in an additive
   closure at `plans/closure/tool-selection-advisor-causal-frontier-timing-corrective/003-status.md`.
   Reconcile the parent corrective status and registry only after qualification.

## 4. Closure and stop conditions

Close only after a green hosted `CI / verify` on the exact source head with the
full workspace test suite completed, including `replay_p95_within_budget`.
The threshold and assertion must remain present and unchanged. If the test
still breaches when isolated, stop and open a separate measurement/code
investigation; do not raise the budget or classify the failure as harmless
noise.

Closure: `plans/closure/tool-selection-advisor-causal-frontier-timing-corrective/003-status.md`.
Hosted CI run `38850114895` passed on the qualified source head; the replay p95
test passed in the exclusive group and all 13,961 workspace tests passed.

