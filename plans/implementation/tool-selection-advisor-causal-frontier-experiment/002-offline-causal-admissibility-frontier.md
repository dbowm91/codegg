# Tool-Selection Advisor Causal Frontier M002 — Offline Causal Admissibility Frontier

Status: blocked on positive M001

Repository baseline: `aa21cfe1763d7ea00d11e582ed4108233e4088a9`

Source roadmap:

- `plans/subsystems/tool-selection-advisor-causal-frontier-experiment-roadmap.md#m002--offline-causal-admissibility-frontier`

Primary class: architecture experiment.

## 1. Objective

Determine whether deterministic precondition filtering over host-owned state can
reduce premature tool exposure while preserving every current-step tool.

No provider request or live tool palette changes in M002.

## 2. Frontier semantics

For each tool already present in `ResolvedToolSurface`:

- hidden/denied/disabled/non-callable/parent-ceiling filtering has already
  happened upstream and remains authoritative;
- contracted tool:
  - evaluate `requires_all`;
  - evaluate each preregistered `requires_any` group;
  - reject from the causal **promotion** frontier if a forbidden fact is
    present;
- uncontracted tool:
  - never enters the causal promotion frontier;
  - remains in the ordinary deferred/discovery universe.

Required/never-reduce tools bypass causal suppression and remain visible.

The output is a visibility recommendation, not call authorization.

## 3. Fail-safe result type

Use a typed result such as:

```rust
struct CausalFrontier {
    admissible_contracted: BTreeSet<String>,
    inadmissible_contracted: BTreeMap<String, Reason>,
    uncontracted_fallback: BTreeSet<String>,
    required_visible: BTreeSet<String>,
    snapshot_fingerprint: String,
    contract_catalog_fingerprint: String,
}
```

Every decision must be explainable by typed facts.

## 4. Baselines

Compare on frozen M001 dev only:

1. current contextual-immediate palette behavior;
2. no causal filter / full eligible deferred universe;
3. frozen Signal V2 lexical ranking where a semantic ranking comparison is
   useful;
4. M002 causal admissibility.

Do not retrain or alter any historical retrieval model.

## 5. Metrics

Report:

- gold current-step coverage;
- gold premature/inadmissible exposure;
- contracted frontier size p50/p95/max;
- total visible-tool count when projected onto current core palette;
- uncontracted fallback retention;
- mutating/shell tool exposure;
- required/never-reduce preservation;
- state-family metrics;
- authority violations;
- p50/p95/max pure frontier evaluation latency.

Also report whether each of the historical 53 inferable retrieval labels would
be contracted, uncontracted, admissible or unavailable. This is diagnostic
only; the causal benchmark, not the old retrieval corpus, owns selection.

## 6. Qualification partition discipline

After selecting no tunable parameters (the contract semantics are frozen in
M001), run the untouched M001 qualification partition once.

Do not edit contracts after seeing qualification results.

If a contract error is discovered, disposition E and a corrective is required.

## 7. Disposition

### A — positive causal-admissibility architecture

All frozen M001 gates pass on qualification:

- current-step preservation =1.00;
- authority violations=0;
- uncontracted fallback=1.00;
- premature exposure reduction>=0.50;
- median structured-state frontier<=4;
- latency gate passes.

M004 becomes ready for observe integration.

M003 may additionally run as an optional effect-path experiment.

### D — no useful structural reduction

If correctness passes but the reduction/menu-quality gates fail, close the
workstream negative. Do not add semantic parsing or learned contracts to rescue
M002.

### E — contract/state correctness failure

Stop and register a corrective.

## 8. Regression tests

- required tool cannot be removed;
- uncontracted tool stays discoverable;
- missing state fact yields deterministic inadmissibility only for contracted
  promotion;
- contradictory state does not panic;
- surface order does not affect frontier;
- state/contract fingerprint drift invalidates cached result;
- no authority widening;
- no provider-definition mutation in M002.

## 9. Verification

```bash
cargo test -p codegg --lib tool_advisor::causal_frontier
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
```

## 10. Acceptance

M002 closes A/D/E with a machine-readable result.

Only A opens M004. M003 is optional after A.
