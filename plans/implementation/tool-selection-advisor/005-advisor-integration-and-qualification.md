# Tool-Selection Advisor Milestone 005 — Advisor Integration and Qualification

Status: active

Repository baseline: `ed960f2ae7043acd816970f7d06e74dc69b09ae8`

Source roadmap:

- `plans/subsystems/tool-selection-advisor-roadmap.md#m005--advisor-integration-and-downstream-qualification`

Applicable ADRs:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Primary class: capability

Hard dependencies: M002, M003, and M004 accepted closure.

## 1. Objective

Integrate the qualified local advisor with existing discovery in controlled experimental modes and determine whether it materially improves tool use, especially for smaller primary models, without changing CodeGG's default behavior or execution authority.

## 2. Why this milestone is blocked

Active integration is not justified until the runtime, locally trained artifact, calibration, and data lifecycle exist and have closure evidence.

## 3. Current implementation evidence

The established path already supports compact two-stage `tool_search`, BM25/keyword retrieval, and progressive disclosure. M005 should add only an advisory ranking layer and an optional promotion projection over the already-allowed surface.

## 4. Invariants that must not regress

- Default mode remains `off`.
- `off` is behaviorally equivalent to no advisor build/model.
- Reranking cannot add candidates absent from policy-allowed search.
- Promotion cannot resurrect denied/hidden/disabled/plan/parent-ceiling-ineligible tools.
- Advisor never supplies execution arguments.
- Advisor failure/timeouts fall back to current ordering/disclosure.
- Telemetry choices are independent of advisor mode.

## 5. Scope

### In scope

- `observe`: score only, no model-facing change.
- `rerank`: rerank current broad `tool_search` candidates.
- `promote`: optionally promote a small bounded number of high-confidence deferred allowed tools into current-turn advertisement.
- Threshold/abstention calibration.
- Side-by-side benchmark against keyword/BM25.
- Downstream trajectories using representative small/tool-fragile primary models.
- Prompt-token, latency, task-success, unnecessary-tool, shell-fallback, and error measurements.
- Unknown MCP/plugin descriptor tests.
- Operator diagnostics for why a tool was promoted/ranked.

### Explicitly out of scope

- Default-on learned advice.
- Removing `tool_search`/BM25 fallback.
- Automatic tool execution.
- Model routing/planning/memory decisions; those require separate workstreams.
- Remote inference.

## 6. Required production changes

### Reranking

Use the existing candidate generator for large catalogs, then apply learned scoring to the bounded shortlist. Preserve stable deterministic tie/fallback ordering.

### Promotion

Promotion occurs after authority/policy filtering and before provider definitions are finalized. Limit count and added schema/token budget. Strong abstention/thresholding is required; uncertain advice leaves the surface unchanged.

The prediction record should include model version, surface fingerprint, candidates/scores, threshold, selected action, and latency.

### Qualification

Pre-register the evaluation matrix before tuning thresholds on final test tasks. At minimum compare:

- no advisor/current configuration;
- keyword;
- BM25;
- learned observe results;
- learned rerank;
- learned promote.

Report results by primary-model capability tier rather than only aggregate.

## 7. Ordered work packages

A. Observe-mode live trajectory harness.
B. Rerank integration with exact policy/fallback tests.
C. Promotion projection with strict budget/authority tests.
D. Calibration/threshold selection on development data only.
E. Downstream A/B qualification with small main models.
F. Operator/config documentation and experimental labeling.

## 8. Failure, cancellation, restart, and contention semantics

Advisor failure immediately reuses current deterministic discovery. No advisor retry loop may delay a turn materially. Promotion is turn-local and reconstructs from current resolved surface after restart/config changes.

## 9. Compatibility and migration

No provider protocol or storage migration. Older models that satisfy M002 artifact compatibility may run in observe mode; rerank/promote may require a minimum qualified artifact capability/version.

## 10. Required tests

- off/observe do not change tool definitions/order;
- rerank preserves candidate set;
- promote is subset of allowed deferred universe;
- denied/hidden/disabled/plan/parent-ceiling negatives;
- low confidence/no-tool -> no promotion;
- unknown textual MCP/plugin tool ranking;
- model error/timeout -> deterministic fallback;
- prompt-budget cap;
- telemetry off remains off in every advisor mode.

## 11. Required verification commands

In addition to focused unit/integration tests:

```bash
cargo run -- tool-advisor bench --dataset <held-out>
cargo run -- tool-advisor qualify --model <artifact> --suite <downstream-suite>
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

Use actual implemented command names in closure.

## 12. Documentation updates

- mode semantics and experimental status;
- threshold/promotion budgets;
- fallback behavior;
- benchmark/qualification report;
- how to disable/uninstall model assets.

## 13. Acceptance criteria

M005 closes when:

1. `off` remains the default and is proven equivalent to current behavior;
2. learned reranking is at least non-inferior to BM25 on primary held-out ranking metrics and demonstrates a documented improvement on at least one predeclared hard/unknown-tool metric;
3. promotion has strong no-tool/false-promotion behavior and cannot widen authority;
4. downstream small-model trajectories show whether the advisor improves task/tool behavior; if they do not, `rerank/promote` remain unqualified/observe-only and closure records that result honestly;
5. resource/token overhead is measured and bounded;
6. failure fallback and telemetry independence are proven.

## 14. Stop conditions

Stop if improvement requires weakening policy, exposing every full schema, running remote inference, tuning on the final test split, or making advisor use/telemetry default-on.

## 15. Closure evidence required

- complete benchmark matrix and dataset fingerprints;
- calibration/threshold procedure;
- unknown-tool results;
- downstream task/tool metrics by primary-model tier;
- CPU/resource/prompt overhead;
- authority-negative/fallback tests;
- recommendation limited to supported experimental modes, without changing the roadmap's default-off contract.

## 16. Handoff notes

A result that the tiny model does not help is valid evidence. Do not hide a failed experiment by widening the model or turning the milestone into general agent redesign.
