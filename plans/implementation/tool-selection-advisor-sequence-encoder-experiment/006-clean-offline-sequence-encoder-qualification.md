# Tool-Selection Advisor Sequence-Encoder Experiment M005 — Clean Offline Sequence-Encoder Qualification

Status: ready for handoff

Repository baseline: `8487967d9cc2605c398d3c608e353c8871289a7d`

Hard dependencies:

- P001 evidence/preregistration polish;
- M002 AdvisorContextV2;
- M003 positive encoder experiment;
- M004 hybrid retrieval experiment.

Source roadmap:

- `plans/subsystems/tool-selection-advisor-sequence-encoder-experiment-roadmap.md#m005--clean-offline-sequence-encoder-qualification`

Primary class: qualification/closure.

## Objective

Run one final, preregistered offline evaluation that decides whether the sequence-encoder architecture is worth live-primary-model testing.

No provider calls occur in M005.

## Two-commit protocol

This is mandatory for a positive qualification claim.

### Commit A — preregistration freeze

Commit:

- dataset/split fingerprints;
- selected model/tokenizer/source-weight hashes;
- training config hashes;
- selected retrieval mode/K;
- promotion thresholds/budgets;
- primary metrics;
- pass/fail gates;
- resource limits;
- exact evaluation command.

Hosted CI for Commit A must pass before final evaluation starts where CI is available.

### Commit B — final evaluation and closure

Run the frozen protocol exactly, commit machine-readable results and closure record, and cite Commit A.

No gate/config/model changes are allowed after final-test inspection. Any required change creates a new preregistration commit and invalidates the prior final run.

## Comparison arms

At minimum:

- keyword;
- BM25;
- `hashed-linear-v1`;
- rejected `contextual-embedding-v2` for historical comparison;
- selected pairwise/packed sequence-encoder variant;
- hybrid retrieval + selected ranker;
- no-advisor disclosure baseline.

## Frozen evaluation slices

Use the C001 frozen final test/family holdouts plus deterministic AdvisorContextV2 projection:

- aggregate test;
- counterfactual pairs;
- hard negatives;
- no-tool/abstention;
- unknown/renamed tools;
- plugin/LSP/research/structured true family holdouts;
- 64-tool and expanded 128-tool retrieval fixtures;
- long-session/stale-origin context-v2 fixtures added before preregistration.

## Primary gates

Unless Commit A explicitly tightens them:

- zero leakage violations;
- zero authority-negative promotion violations;
- candidate recall >=0.98 at selected K on 64-tool fixture and >=0.95 on 128-tool fixture;
- aggregate test MRR not worse than `hashed-linear-v1` by >0.01;
- sequence encoder improves MRR or Recall@1 by >=0.02 on at least one contextual slice without >=0.02 regression on any other predeclared contextual slice;
- unknown-tool/family-holdout performance not worse than linear by >0.02;
- no-tool F1 not worse than best nontrivial baseline by >0.02;
- calibrated Brier/ECE non-inferior to the same encoder uncalibrated;
- proactive promotion has useful recall at a tolerable false-promotion rate;
- default/off/no-model behavior remains bit-for-bit/semantically equivalent;
- resource limits from Commit A are met.

## Resource report

Record:

- model and tokenizer bytes;
- binary delta;
- cold load;
- warm/cold cache memory;
- CPU p50/p95 query+retrieval+rank latency;
- Apple Silicon accelerated latency if available;
- RSS;
- K scaling;
- number of encoder forwards per turn;
- prompt/schema bytes added by promotions.

Do not compare resource numbers across different hardware without identifying the target.

## Disposition

Choose exactly one:

- **A — qualify for live M004:** all gates pass; registry may mark the existing live-primary-model M004 ready subject to its original provider/resource prerequisites.
- **B — quality gain but deployment cost too high:** keep model research-only; M004 blocked.
- **C — retrieval useful, ranker not qualified:** retain retrieval only if it independently improves normal discovery and has its own safe integration plan; M004 blocked.
- **D — no useful gain:** close experiment; M004 blocked.
- **E — correctness/framework failure:** register a narrow corrective only if the failure is implementation correctness rather than a negative model result.

## Verification

Before final run:

```bash
cargo test --workspace --locked
cargo test --locked --features tool-advisor-encoder-experiment
cargo test --locked --features tool-advisor-encoder-training
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

Record hosted CI for both preregistration and closing commits when available.

## Acceptance

M005 closes when the frozen protocol is reproducible, all metrics/resources are recorded, and one explicit disposition is registered. A negative disposition is valid closure.
