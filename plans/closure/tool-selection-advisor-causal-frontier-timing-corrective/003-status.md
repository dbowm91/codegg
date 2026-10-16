# Tool-Selection Advisor Timing Corrective C003 — Closure

Status: closed — replay p95 measurement isolated from concurrent CI binaries

Source plan: `plans/implementation/tool-selection-advisor-causal-frontier-timing-corrective/003-isolate-replay-p95-measurement.md`
Implementation: `b6d334fe`
Qualified source head: `3d395876db3d88bad692596c15f892b0494a165c`
Hosted CI: [`38850114895`](https://github.com/dbowm91/codegg/actions/runs/38850114895) — success

## 1. Closure finding

The `causal_observe_replay` binary now joins the existing `threads-required =
"num-cpus"` nextest group. The retained 201-sample p95 test passed in that
exclusive slot, and the full workspace suite passed. The 5.0 ms constant and
assertion are unchanged. No test was skipped or moved to a non-default CI lane.

## 2. Evidence

| Requirement | Evidence | Result |
|---|---|---|
| Test has exclusive CPU scheduling | `.config/nextest.toml` CI override includes `binary(causal_observe_replay)` alongside the existing whole-binary exclusive targets | pass |
| Frozen p95 threshold and assertion retained | `CAUSAL_OBSERVE_P95_BUDGET_MS` remains 5.0 ms; `tests/causal_observe_replay.rs::replay_p95_within_budget` remains unchanged | pass |
| Full canonical CI completes | Hosted run `38850114895`, exact source head, all repository guards and Clippy green; **13,961 passed, 0 failed, 6 skipped** | pass |

The preceding exact-head failures `38840695582` and `38842540498` measured
5.559 ms and 5.597 ms while the `ci` profile ran concurrent binaries. Those
failures motivated the exclusive slot; they are preserved as evidence and do
not alter the frozen gate.

## 3. Scope and unblock audit

No production code, advisor behavior, model contract, or threshold changed.
The full workspace suite and blackboard trajectory were able to complete.
This corrective unblocks no separate advisor milestone; C001/C002 historical
closure records remain unchanged.

