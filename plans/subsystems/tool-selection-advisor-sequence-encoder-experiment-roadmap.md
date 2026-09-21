# Tool-Selection Advisor — Sequence Encoder and Retrieval Experiment Roadmap

Status: active

Repository planning baseline: `8487967d9cc2605c398d3c608e353c8871289a7d`

Predecessor evidence:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md` — controlling architecture decision.
- `plans/subsystems/tool-selection-advisor-evidence-integrity-corrective-addendum.md` — closed with C004 disposition B.
- `plans/closure/tool-selection-advisor-evidence-integrity-corrective/004-status.md` — clean offline qualification showing the hashed embedding contextual scorer does not provide useful gain.
- `architecture/tool-advisor.md` — current runtime/training/consent/disclosure architecture.
- `architecture/tool-advisor-framework-spike.md` — historical M002 framework decision; requires polish because the selected custom scorer was later demoted to a research baseline.

Long-term references:

- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#46-progressive-disclosure`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/003-planning-process.md`

## 1. Purpose

The evidence-integrity corrective established trustworthy data, training mathematics, calibration machinery, authority-safe pre-turn disclosure, and a reproducible offline qualification harness. Its final result was negative for the current model architecture:

- `hashed-linear-v1`: test MRR 0.707;
- corrected contextual hashed-embedding variants: 0.457–0.595 MRR;
- contextual abstention/calibration did not transfer well;
- BM25 deferred preselection at budget 16 reached only 0.8286 relevant-tool recall;
- live primary-model qualification correctly remained blocked.

This roadmap therefore starts a **new model-architecture experiment** rather than reopening the closed corrective.

The experiment will test a real pretrained bidirectional sequence encoder in the intended approximately 5M–25M class, with textual tool descriptors and explicit abstention, while preserving CodeGG's pure-Rust/local/optional runtime constraints. It will also repair the coarse-retrieval bottleneck so a better ranker is not starved of the relevant tool.

## 2. Research basis

Current public reference points make a sequence-encoder experiment technically credible:

- TinyBERT 4-layer / hidden 312 checkpoints are about 14.4M parameters and fit the target capacity class.
- MiniLM L6/H384 is about 22M parameters and is a well-established compact semantic encoder reference.
- Candle provides Rust-native model training, BERT implementations, safetensors loading, CPU support, macOS acceleration, and optional Metal.
- Burn provides Rust-native train/infer, safetensors import, CPU and Apple Metal backends.
- Recent Burn Metal reports include a params-only backward NaN failure mode, so frozen-encoder/head-only fine-tuning on Apple Silicon must be explicitly qualified rather than assumed.

Reference links are informational and do not become runtime dependencies:

- https://huggingface.co/NatureUniverse/TinyBERT_general_4L_312d
- https://huggingface.co/microsoft/xtremedistil-l6-h384-uncased
- https://github.com/huggingface/candle
- https://github.com/tracel-ai/burn
- https://github.com/tracel-ai/burn/issues/5162

## 3. Durable invariants

- Advisor use remains optional and default-off.
- Normal CodeGG works without model weights, tokenizer assets, ML framework features, training features, or network access.
- Runtime inference and tokenization remain in-process Rust.
- Model/training assets are explicit local inputs with hashes, provenance, and license metadata; CodeGG does not silently download them.
- `ResolvedToolSurface` remains the sole authority source.
- Advisor output changes ranking/visibility only; it never grants permission, executes tools, or constructs tool arguments.
- Unknown/new tools are represented by textual descriptors, never fixed classifier IDs.
- Final test/family-holdout data remains untouched by training, threshold selection, architecture selection, and calibration.
- The frozen C001 dataset fingerprints remain the primary continuity anchor; new training-only data may be added only with leakage isolation from frozen dev/test.
- Historical closure records remain immutable.
- Existing live-primary-model M004 remains blocked unless this roadmap's final offline qualification records a positive disposition.

## 4. Architecture hypotheses

### H1 — pairwise cross-encoder

For each shortlisted tool:

```text
[CLS] bounded current task state [SEP] tool descriptor [SEP]
```

Use the pooled/[CLS] representation for relevance plus an explicit no-tool/abstention head.

Advantages: strongest direct task/tool interaction, simplest correctness story.
Risk: N encoder forwards per candidate set.

### H2 — packed marker ranker

Laya-inspired single-pass layout:

```text
[CLS] bounded current task state [SEP]
[M] descriptor-1 [M] descriptor-2 ... [SEP]
```

Score hidden states at marker positions and derive abstention from pooled state plus top-score/margin features.

Advantages: one encoder forward for the shortlist, directly aligned with typed-choice selection.
Risk: sequence-budget pressure and more custom head logic.

### H3 — semantic coarse retriever

Encode the current task state once and cache tool-descriptor embeddings by descriptor/surface fingerprint. Combine semantic similarity with BM25 using deterministic union/RRF, then feed the selected shortlist to H1/H2.

Advantages: fixes the measured 0.83 BM25 recall ceiling without scoring every candidate with the expensive cross-encoder.
Risk: additional cache/artifact complexity and possible objective mismatch.

## 5. Training strategy

The experiment does not start by full-fine-tuning 14–22M parameters on 132 train cases.

For each encoder candidate, use staged training:

1. frozen pretrained encoder + learned ranking/abstention head;
2. unfreeze the top one or two transformer layers only if stage 1 shows signal;
3. full fine-tuning only if the prior stages are insufficient and dev evidence justifies it.

This isolates whether pretrained semantics solve the problem before adding overfitting capacity.

Any larger training-only corpus must be separate from frozen C001 dev/test and pass the same content/template leakage machinery. Remote teacher calls are not automatic and are out of scope for this roadmap.

## 6. Dependency graph

```text
P001 evidence/preregistration polish ----+
                                         |
M001 framework spike (closed) --+
                                |
M001A real reference checkpoint (conditional) --+
                                                |
M001B encoder fine-tuning corrective (closed) --+--> M003 sequence encoder experiment
                                                 |       (ready: all stages via
                                                 |        the M001B path)
M002 current-state context v2 ------------------+--> M003 sequence encoder experiment
                                                    |
                                                    v
                                          M004 hybrid retrieval experiment
                                                    |
                                                    v
                                          M005 clean offline qualification
                                                    |
                                         positive only
                                                    v
                                  existing live-primary-model M004
```

- P001 and M002 are closed.
- M001 is closed: Candle 0.11 selected and the real MiniLM reference asset
  is qualified. A newly discovered staged-backward weakness (fused
  `layer_norm` has no backward) is owned by M001B, not M001.
- M001A is conditionally closed: provenance, complete load, deterministic
  forward, head-only training, and pooling are qualified; encoder
  unfreezing is the named outstanding condition.
- M001B is the narrow framework corrective that restored differentiable
  encoder fine-tuning (closed positively via the composite-norm path with
  forward parity and correctly-scoped update evidence).
- M003 is ready for all three training stages; its top-layer/full
  unfreeze stages run through the M001B differentiable path.
- M004 requires M003 because it reuses the selected encoder/tokenizer contract.
- M005 requires P001 + M002 + M003 + M004.
- Existing post-closure M004 remains blocked until M005 records a positive disposition.

## 7. Milestones

### P001 — Evidence-state and preregistration polish

Plan:

- `plans/implementation/tool-selection-advisor-sequence-encoder-experiment/001-evidence-preregistration-polish.md`

Status: closed.

Reconcile stale C004/framework documentation, add the now-successful hosted CI evidence as a supplemental record without rewriting historical closure, and make separate-commit preregistration a closure requirement for future qualification.

### M001 — Rust sequence-encoder framework and local-asset spike

Plan:

- `plans/implementation/tool-selection-advisor-sequence-encoder-experiment/002-rust-sequence-encoder-framework-asset-spike.md`

Status: closed (Candle selected; MiniLM reference asset qualified by M001A).

Candle 0.11 and the local manifest/forward/backward contract are
implemented, and the M001A reference checkpoint loads completely with
deterministic forward and scoped head-only training. A newly discovered
framework weakness — Candle 0.11 fused `layer_norm` records no backward,
so no encoder weight can be trained — is owned by M001B, not M001.

### M001A — Reference checkpoint materialization and qualification

Plan:

- `plans/implementation/tool-selection-advisor-sequence-encoder-experiment/007-reference-checkpoint-materialization-and-qualification.md`

Status: conditionally closed.

One pinned real MiniLM-class safetensors checkpoint
(`sentence-transformers/all-MiniLM-L6-v2@1110a24…`) is materialized
outside Git with recorded provenance/license/hashes, loads completely
into Candle (101/101 variables, zero missing), runs deterministically,
trains a scoped ranking head, and shows CLS/mean pooling separation
(mean margin +0.26 vs CLS +0.11 on the train/dev-only sanity set).
Closure: `plans/closure/tool-selection-advisor-sequence-encoder-experiment/007-status.md`.

Named condition: encoder unfreezing is impossible on Candle 0.11 (fused
`layer_norm` has no backward). M003 stage 1 is unblocked; unfreeze
stages are gated on M001B.

### M001B — Encoder fine-tuning framework corrective

Plan:

- `plans/implementation/tool-selection-advisor-sequence-encoder-experiment/008-encoder-finetuning-framework-corrective.md`

Status: closed (positive option-1 disposition in
`plans/closure/tool-selection-advisor-sequence-encoder-experiment/008-status.md`).

Restore differentiable encoder fine-tuning on the qualified MiniLM asset
(composite-norm forward with forward parity and correctly-scoped update
evidence): CodeGG-owned composite-norm encoder, 101/101 weights, parity
max 2.3e-06, correctly-scoped top-layer updates. M003 stages 2–3 are
authorized through this path.

### M002 — Current-state advisor context projection v2

Plan:

- `plans/implementation/tool-selection-advisor-sequence-encoder-experiment/003-current-state-advisor-context-v2.md`

Status: closed.

Replace original-session-prompt-only advisor context with a versioned bounded projection of the current objective/task state while preserving privacy and no-history-service constraints.

### M003 — True sequence-encoder ranking experiment

Plan:

- `plans/implementation/tool-selection-advisor-sequence-encoder-experiment/004-sequence-encoder-ranking-experiment.md`

Status: closed (packed-marker head-only variant selected; top-layer attempt
stopped at the measured local resource bound).

Implement pairwise and packed-marker ranking heads over the qualified
local pretrained MiniLM encoder with the encoder frozen, train the
ranking/abstention head, calibrate on dev only, and compare on clean
held-out slices. Pooling strategy (`cls` vs `mean`) is an explicit
per-run parameter following the M001A finding. Top-layer/full unfreeze
stages run through the M001B composite-norm encoder. M003 selected the
packed-marker head-only artifact: dev MRR 0.823 versus pairwise 0.595 and
BM25 0.515. The top-layer attempt exceeded the local ten-minute resource
bound without producing an artifact, so full fine-tuning was not justified.

### M004 — Hybrid semantic/BM25 candidate retrieval

Plan:

- `plans/implementation/tool-selection-advisor-sequence-encoder-experiment/005-hybrid-retrieval-shortlist-experiment.md`

Status: blocked on M003.

Use the selected encoder as a cached semantic coarse retriever and combine it with BM25 over the complete allowed deferred universe. Search the K=16/24/32 quality/latency frontier rather than hard-coding K=16.

### M005 — Clean offline sequence-encoder qualification

Plan:

- `plans/implementation/tool-selection-advisor-sequence-encoder-experiment/006-clean-offline-sequence-encoder-qualification.md`

Status: blocked on P001 + M002 + M003 + M004.

Pre-register in a separate commit, then run final frozen evaluation comparing baselines, sequence-encoder variants, hybrid retrieval, calibration, resources, and authority invariants. Only a positive result may unblock live M004.

## 8. Completion definition

The workstream closes with one explicit disposition:

- **A — qualify:** sequence encoder + retrieval clears offline gates and live M004 becomes ready subject to its original provider/resource prerequisites;
- **B — model gain but deployment cost too high:** retain research implementation and keep live M004 blocked;
- **C — retrieval fixed but ranker not useful:** retain retrieval improvements only if independently valuable and keep learned promotion blocked;
- **D — no useful gain:** retain infrastructure/research artifacts as appropriate and close without live testing;
- **E — framework/runtime incompatibility:** record the failed experiment and do not weaken ADR-0009 to force adoption.

A negative result is acceptable closure evidence.
