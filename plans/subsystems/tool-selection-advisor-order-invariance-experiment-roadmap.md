# Tool-Selection Advisor Order-Invariance Experiment Roadmap

Status: active

Repository planning baseline: `198524aa4ff8656928c86cf36168892532f2e29c`

Controlling architecture:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Predecessor evidence:

- `plans/closure/tool-selection-advisor-qualification-evidence-corrective/002-status.md` — semantically valid v3 disposition D.
- `assets/tool-advisor/sequence-qualification-v3-result.json` — diagnostic evidence only for this new workstream.
- `plans/closure/tool-selection-advisor-sequence-encoder-experiment/004-status.md` — original sequence-ranker experiment.
- `plans/closure/tool-selection-advisor-sequence-encoder-experiment/008-status.md` — differentiable MiniLM/Candle fine-tuning path.

## 1. Purpose

The v3 qualification closed the evidence-integrity question and exposed a concrete modeling failure instead of another harness defect.

The selected packed-marker MiniLM ranker is not candidate-order robust:

- historical corpus: 207 non-no-tool cases;
- highest-relevance candidate at index 0 in 201/207 cases (97.1%);
- highest-relevance candidate at index 1 in the remaining 6/207;
- v3: 146 non-no-tool cases;
- highest-relevance candidate at index 2 in 133/146 cases and index 3 in 13/146;
- v3 sequence Recall@1: 0.0;
- v3 linear Recall@1: 0.60;
- v3 counterfactual pair accuracy: 0/13.

The implementation explains why the shortcut was learnable:

- packed encoding assigns candidate position N a distinct `[unusedN]` marker token;
- BERT absolute position embeddings also expose descriptor position;
- training loss reduces each non-no-tool case to one target index;
- the historical corpus almost always places that target first;
- multi-tool graded relevance is therefore not fully represented by the objective.

Two independent operating-point failures also remain:

- 64/128-tool retrieval recall at K=16 is 0.9294, below 0.98/0.95 gates;
- proactive promotion reuses the dev abstention threshold as an absolute candidate-score threshold even though these are different statistical quantities.

This workstream tests order-robust alternatives and fixes the training/operating-point contract. It does not reopen the v3 corrective or reinterpret its D disposition.

## 2. Durable invariants

- Advisor remains optional, local, default-off, and advisory-only.
- Normal CodeGG works without any model artifact.
- No runtime model download path.
- Production inference/tokenization remains in-process Rust.
- `ResolvedToolSurface` remains the sole authority source.
- Candidate permutation MUST NOT alter authority or candidate identity.
- Training augmentation is local-only and never creates remote telemetry.
- Historical train/dev/test, v2, and v3 corpora remain immutable.
- v3 may be used as diagnostic evidence after model selection, but MUST NOT select architecture, hyperparameters, thresholds, retrieval K, or promotion operating point.
- Final positive qualification requires a fresh v4 holdout unseen by the selected model and all selection logic.
- Historical closure records remain immutable.

## 3. Root-cause hypotheses

### H1 — Candidate-order shortcut

Distinct marker identities plus absolute positions allow the packed head to associate "early candidate" with relevance. Extreme historical target-position skew makes that shortcut easier than learning task↔descriptor semantics.

### H2 — Single-target objective loses graded information

Current training chooses one `target_index` using maximum relevance and applies ordinary cross entropy. Multi-tool and graded labels are therefore compressed into one class target. A listwise graded objective plus explicit binary candidate relevance is more faithful to the data contract.

### H3 — Promotion confidence is not calibrated

The current promotion path compares raw candidate score to a threshold. The v3 protocol reused the abstention decision threshold as the promotion threshold. Candidate relevance and abstention need separate calibration/operating points.

### H4 — K=16 is too narrow for large catalogs

The corrected v3 retriever recovered 158/170 labeled relevant tools at both 64- and 128-tool universes. A dev-only K/mode frontier may recover the missing recall without changing authority semantics.

## 4. Architecture candidates

The experiment compares, at minimum:

1. **Current distinct-marker packed ranker** — negative control only.
2. **Shared-marker packed ranker** — one marker identity for all descriptors; descriptor spans remain distinct by sequence position.
3. **Shared-marker span-pooled packed ranker** — candidate representation is mean/attention pooled over each descriptor span rather than a position-specific marker alone.
4. **Batched pairwise cross-encoder** — each candidate is encoded as `[CLS] context [SEP] descriptor [SEP]`; candidate rows are batched into one BERT forward when feasible. This is the order-equivariant reference because candidate representation does not depend on candidate-list ordinal identity.

The experiment may retain fewer variants if a prerequisite property test proves a variant incapable of satisfying the order-invariance contract.

## 5. Training strategy

Training-only augmentation derives deterministic candidate permutations from the existing clean train partition.

For each source training case:

- preserve the same context, candidates, labels, leakage group, and provenance;
- produce balanced target-position views across valid candidate positions;
- permute no-tool cases too, so order is never a no-tool cue;
- preserve graded relevance by candidate name;
- never create a new semantic split group merely because order changed.

Recommended objective:

- listwise graded-relevance loss;
- binary candidate relevance loss for absolute promotion confidence;
- abstention BCE;
- optional permutation-consistency loss after scores are mapped back to canonical candidate identity.

The exact weights are dev-selected only and must be recorded.

## 6. Dependency graph

```text
M001 diagnostics + permutation contract
             |
             v
M002 order-robust ranker architectures
             |
             v
M003 balanced training + dev selection
             |
             v
M004 retrieval/promotion operating point
             |
             v
M005 fresh v4 preregistered qualification
             |
          disposition A
             v
existing live-primary-model M004
```

- M001 is ready.
- M002 is blocked on M001.
- M003 is blocked on M002.
- M004 is blocked on positive M003 selection.
- M005 is blocked on positive M004 closure.

## 7. Milestones

### M001 — Candidate-order diagnostics and permutation contract

Plan:

- `plans/implementation/tool-selection-advisor-order-invariance-experiment/001-candidate-order-diagnostics-and-permutation-contract.md`

Status: ready.

Make positional shortcut evidence reproducible, add permutation-invariance metrics, and define deterministic train/dev-only permutation augmentation without changing historical corpora.

### M002 — Order-robust ranker architectures

Plan:

- `plans/implementation/tool-selection-advisor-order-invariance-experiment/002-order-robust-ranker-architectures.md`

Status: blocked on M001.

Implement shared-marker/span-pooled packed variants and a batched pairwise cross-encoder reference with architecture-level permutation property tests.

### M003 — Balanced training and dev selection

Plan:

- `plans/implementation/tool-selection-advisor-order-invariance-experiment/003-balanced-training-and-dev-selection.md`

Status: blocked on M002.

Train on balanced permutation views using graded/listwise + candidate-relevance + abstention objectives; select one model using dev quality plus permutation robustness, never v3.

### M004 — Retrieval and promotion operating point

Plan:

- `plans/implementation/tool-selection-advisor-order-invariance-experiment/004-retrieval-and-promotion-operating-point.md`

Status: blocked on M003.

Choose K/mode and a promotion-specific calibrated confidence threshold on train/dev-only fixtures. Keep abstention calibration separate.

### M005 — Fresh v4 preregistered qualification

Plan:

- `plans/implementation/tool-selection-advisor-order-invariance-experiment/005-fresh-v4-preregistered-qualification.md`

Status: blocked on M004.

Freeze architecture/model/operating point plus a fresh v4 semantic holdout in a separate preregistration commit, require CI, then perform one release-mode final evaluation.

## 8. Exit conditions

A successful workstream requires:

- candidate-order invariance/equivariance proven on dev permutation suites;
- no target-index performance collapse;
- meaningful graded/multi-tool behavior;
- promotion confidence distinct from abstention calibration;
- large-catalog retrieval targets satisfied at a bounded K;
- fresh-v4 quality/generalization gates passed;
- authority/resource/default-off invariants preserved.

A negative M005 result closes the experiment and leaves live M004 blocked. Only M005 disposition A may make live M004 dependency-ready.
