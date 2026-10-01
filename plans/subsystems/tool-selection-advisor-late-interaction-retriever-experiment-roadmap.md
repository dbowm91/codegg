# Tool-Selection Advisor Late-Interaction Retriever Experiment Roadmap

Status: active — M001 blocked on retrieval-signal closure corrective C001

Repository planning baseline: `c4cc6c5b3c9154428b8560a60aa0129624435dfe`

Controlling architecture:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Hard predecessor / cleanup dependency:

- `plans/subsystems/tool-selection-advisor-retrieval-signal-closure-corrective-addendum.md`
- `plans/implementation/tool-selection-advisor-retrieval-signal-closure-corrective/001-terminal-roadmap-and-ci-reconciliation.md`

Frozen predecessor evidence:

- `plans/closure/tool-selection-advisor-retrieval-evaluation-semantics-corrective/001-status.md`
- `plans/closure/tool-selection-advisor-retrieval-signal-experiment/001r-status.md`
- `plans/closure/tool-selection-advisor-retrieval-signal-experiment/002-status.md`
- `plans/closure/tool-selection-advisor-retrieval-signal-experiment/003-status.md`
- `assets/tool-advisor/retrieval-relevance-v1.json`
- `assets/tool-advisor/retrieval-signal-m002-frontier.json`
- `assets/tool-advisor/retrieval-signal-m003-projection.json`
- `assets/tool-advisor/order-invariance-m003-selection.json`

## 1. Purpose

The pooled-retrieval line is exhausted.

Against the corrected inferable dev target (53 positives):

- deterministic Signal V2 improves the original retrieval failure but tops out at 51/53;
- the original four persistent misses are repaired, so the remaining misses are not the old documentation-only failure;
- 288 frozen-MiniLM pooled-projection arms produce no eligible retrieval artifact;
- the best pooled projection only ties 51/53 at 64/128 candidates and degrades to 48/53 at 256;
- unknown/renamed and name-masked generalization collapse under the learned pooled projections.

The next architecture therefore moves the interaction boundary **before pooling**.

Instead of reducing each query and tool descriptor to one vector, the retriever keeps token-level encoder representations and computes exact late interaction:

```text
score(Q, D) = mean_i max_j cosine(q_i, d_j)
```

where query and descriptor token vectors are L2-normalized and special/padding tokens are excluded.

This workstream tests whether token-level interaction recovers semantic correspondences that a single pooled vector destroys.

## 2. Research basis

External research is informative, not a runtime dependency.

- ColBERTv2 establishes multi-vector token-level late interaction as a distinct retrieval architecture from single-vector dense retrieval:
  - https://aclanthology.org/2022.naacl-main.272/
- ToolRet shows that conventional strong IR models still struggle on tool retrieval and that tool-specific supervision matters:
  - https://arxiv.org/abs/2503.01763
- Tool-REX reports that incomplete tool documentation is a major tool-retrieval bottleneck and uses structured document expansion:
  - https://openreview.net/forum?id=g9D9MgG7iW
- Causal Minimal Tool Filtering is relevant future work because semantic relevance and next-step causal necessity are not identical:
  - https://arxiv.org/abs/2606.06284

CodeGG does not adopt those systems wholesale. The experiment reuses the existing pure-Rust/Candle MiniLM stack, corrected retrieval relevance semantics, Signal V2 text, and authority boundary.

## 3. Architectural thesis

### What changes

- token-level representations survive through scoring;
- relevance is computed by MaxSim rather than pooled cosine;
- descriptor token embeddings are cacheable because they depend only on static tool metadata;
- query encoding runs once per turn;
- exact scoring is used first because the qualified catalog bound is only 64/128/256 tools.

### What does not change

- `ResolvedToolSurface` owns authority;
- the advisor remains optional/default-off/local-only;
- historical corpora and derived relevance view remain immutable;
- existing span-packed downstream ranker remains frozen until a retrieval candidate qualifies;
- no runtime model download;
- no Python/PyTorch/FAISS production dependency;
- no ANN/PLAID/MUVERA index in the first architecture line;
- no per-tool hand-authored aliases.

## 4. Why exact late interaction first

At 256 tools, exact MaxSim is bounded enough to test the representation without confounding ANN/indexing behavior.

The existing `CandleBertSequenceEncoder` already produces sequence hidden states internally. M001 may expose a narrow token-embedding API instead of importing another inference stack.

Only after quality is proven may later work optimize indexing/compression. Quality failure must not be hidden behind an approximate retrieval layer.

## 5. Escalation ladder

```text
retrieval-signal closure corrective C001
                |
                v
M001 frozen exact token-level MaxSim
        | clears gates -----------------------> M004
        | useful signal, below gates
        v
M002 learned token projection + MaxSim
        | clears gates -----------------------> M004
        | near-gate + generalization clean
        v
M003 top-layer MiniLM late-interaction tuning
        | clears gates -----------------------> M004
        | otherwise
        v
terminal negative close

M004 eligible retrieval + ranker/promotion operating point
                |
                v
M005 fresh v4 qualification
```

No step may proceed merely because the preceding implementation exists; its evidence gate must be positive.

## 6. Milestones

### M001 — Frozen exact late-interaction baseline

Plan:

- `plans/implementation/tool-selection-advisor-late-interaction-retriever-experiment/001-frozen-exact-maxsim-baseline.md`

Status: blocked on retrieval-signal closure corrective C001.

Expose bounded token hidden states from the existing encoder, preregister an exact MaxSim contract, and evaluate frozen late interaction with no learned parameters.

### M002 — Learned token projection

Plan:

- `plans/implementation/tool-selection-advisor-late-interaction-retriever-experiment/002-token-projection-training.md`

Status: blocked/conditional on positive architectural signal from M001.

Train only small linear token projections before MaxSim. This is intentionally distinct from the failed pooled-projection experiment.

### M003 — Top-layer late-interaction fine-tuning

Plan:

- `plans/implementation/tool-selection-advisor-late-interaction-retriever-experiment/003-top-layer-late-interaction-finetuning.md`

Status: blocked/conditional on M002 near-gate result with clean generalization.

Fine-tune only the top one or two MiniLM layers plus the selected token projection. Full-model tuning is out of scope.

### M004 — Qualified retrieval + promotion operating point

Plan:

- `plans/implementation/tool-selection-advisor-late-interaction-retriever-experiment/004-retrieval-ranker-promotion-operating-point.md`

Status: blocked on an eligible M001/M002/M003 retrieval candidate.

Freeze K/resources/cache behavior, integrate the frozen span-packed ranker, and qualify candidate-relevance promotion separately from abstention.

### M005 — Fresh v4 qualification

Plan:

- `plans/implementation/tool-selection-advisor-late-interaction-retriever-experiment/005-fresh-v4-preregistered-qualification.md`

Status: blocked on positive M004.

Run one fresh, zero-leakage, release-mode qualification of the complete retrieval + ranker + promotion stack.

## 7. Core gates

Unless a plan explicitly makes a stricter preregistration before measurement:

- universe 64 retrieval recall >= 0.99;
- universe 128 recall >= 0.98;
- universe 256 recall >= 0.95;
- K <= 32;
- authority violations = 0.

Generalization remains first-class:

- unknown/renamed;
- canonical-name masking;
- family holdouts;
- paraphrase/no-exact-verb cases;
- schema-absent candidates.

A quality gain that depends on canonical-name memorization is a negative result.

## 8. Resource posture

The first implementation favors correctness and exactness over ANN complexity.

At 256 tools, descriptor token vectors may be cached in-process. M001/M004 must report:

- token count distribution;
- cache entries and bytes;
- query encode p50/p95;
- exact MaxSim arithmetic p50/p95;
- end-to-end retrieval p50/p95;
- RSS;
- cold load;
- encoder forwards per turn.

No user query/context may enter the persistent descriptor cache.

## 9. Non-goals

This workstream does not implement:

- causal precondition/effect tool filtering;
- large external tool-retrieval models;
- SPLADE-style vocabulary expansion;
- ANN/FAISS/PLAID/MUVERA;
- full MiniLM fine-tuning;
- tool-document LLM expansion;
- production support beyond the qualified catalog bound;
- a new downstream ranker.

Those require separate evidence/planning if needed.

## 10. Terminal outcomes

- If M001 shows no token-level architectural signal, stop before training.
- If M002 shows pooled-style memorization/generalization collapse, stop before encoder tuning.
- If M003 cannot clear quality/generalization/resource gates, close the workstream negative.
- Only an eligible M001/M002/M003 candidate can open M004.
- Only M005 disposition A may make the existing live-primary-model trajectory study dependency-ready, subject to its original operator/provider prerequisites.
