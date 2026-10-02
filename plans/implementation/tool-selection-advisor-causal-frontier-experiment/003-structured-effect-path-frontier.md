# Tool-Selection Advisor Causal Frontier M003 — Structured Effect-Path Frontier

Status: active (optional; M002 closed with disposition A — see `plans/closure/tool-selection-advisor-causal-frontier-experiment/002-status.md`)

Repository baseline: `ffbd0bc9`

Source roadmap:

- `plans/subsystems/tool-selection-advisor-causal-frontier-experiment-roadmap.md#m003--structured-effect-path-frontier`

Primary class: optional architecture experiment.

## 1. Objective

Test a bounded effect-path refinement over the positive M002 admissibility
frontier, but only when CodeGG already has an explicit structured desired
outcome.

M003 does not parse free-form prompts into goals.

## 2. Structured demand sources

Allowed demand sources are host-owned and typed.

Initial mapping may use:

- unmet `WorkEvidenceKind::TestJob` -> TestEvidenceProduced;
- unmet `WorkEvidenceKind::Commit` -> CommitEvidenceProduced;
- unmet `WorkEvidenceKind::Artifact` -> Artifact/Evidence outcome;
- unmet `WorkEvidenceKind::DelegatedRun` or `AgentRun` -> DelegatedRunProduced;
- turn-local LSP preview plus explicit checked-apply state -> WorkspaceMutationProduced
  only when the host state marks that preview as the active next transition.

Do not infer a demand merely because free text contains words such as
"test", "commit", "fix", "search" or "rename".

If no typed demand exists, M003 returns `NoStructuredDemand` and the caller
uses the M002 frontier unchanged.

## 3. Effect graph

Treat admissible contracted tools as directed transitions from current facts to
declared outcomes.

Keep the planner deliberately bounded:

- maximum path depth 3;
- no repeated tool in one path;
- deterministic lexicographic tie-break after path length;
- no probabilistic score;
- no cycle expansion;
- unknown/uncontracted tools are outside the graph and remain fallback
  discoverable.

A path is advisory: it does not execute tools and does not claim effects have
already occurred.

## 4. Integrity

Because effect metadata becomes load-bearing in M003:

- effect catalog fingerprint must bind tool implementation/schema identity;
- stale contract fingerprint fails closed to M002 fallback;
- no external/MCP contract participates unless source-controlled and trusted;
- no tool output may rewrite its own contract;
- effect-path result includes contract provenance in diagnostics.

## 5. Evaluation

Use the M001 benchmark cases that contain a frozen structured desired outcome.

Compare:

- M002 admissibility frontier;
- M003 minimal effect-path frontier.

Metrics:

- gold current-step preservation;
- path existence;
- path length;
- promoted frontier size;
- premature mutating exposure;
- false causal exclusion;
- contract coverage;
- latency.

## 6. Positive criterion

M003 is adopted by M004 only if:

- current-step preservation remains 1.00;
- authority/fallback invariants remain perfect;
- median promoted frontier is at least 25% smaller than M002 on
  structured-demand cases;
- premature mutating exposure is non-increasing;
- no state family loses a gold tool;
- p95 effect-path computation <=5 ms.

Otherwise M003 closes negative and M004 uses M002.

## 7. Non-goals

- no natural-language goal compiler;
- no graph neural network;
- no self-evolving procedural graph;
- no learned contract inference;
- no persistence of a mutable global procedure graph.

Those are separate future lines if static structured demands prove valuable.

## 8. Verification

```bash
cargo test -p codegg --lib tool_advisor::causal_frontier
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
```

## 9. Acceptance

M003 closes positive/negative/E.

Positive selects M003 for M004; negative leaves positive M002 as the selected
frontier.
