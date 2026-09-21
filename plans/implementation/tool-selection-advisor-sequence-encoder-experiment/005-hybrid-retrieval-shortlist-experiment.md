# Tool-Selection Advisor Sequence-Encoder Experiment M004 — Hybrid Retrieval Shortlist Experiment

Status: ready for handoff

Repository baseline: `8487967d9cc2605c398d3c608e353c8871289a7d`

Hard dependency: M003 positive experimental encoder result.

Source roadmap:

- `plans/subsystems/tool-selection-advisor-sequence-encoder-experiment-roadmap.md#m004--hybrid-semanticbm25-candidate-retrieval`

Primary class: retrieval capability experiment.

## Objective

Raise deferred-tool candidate recall above the measured BM25-only 0.8286 at K=16 without reverting to "advertise everything".

The coarse retriever must operate over the complete already-authorized deferred universe and produce a high-recall shortlist for the M003 ranker.

## Architecture

Reuse the selected encoder/tokenizer where practical.

### Semantic index

For each allowed deferred descriptor, compute and cache a descriptor embedding keyed by:

- surface fingerprint;
- canonical name;
- normalized descriptor hash;
- encoder/tokenizer version.

Tool descriptor embeddings may be cached because they are independent of user task content.

Compute one AdvisorContextV2 query embedding per turn.

### Hybrid ranking

Evaluate deterministic combinations:

- BM25 top-K;
- semantic cosine top-K;
- union then reciprocal-rank fusion;
- union then simple normalized-score fusion;
- optional diversity/deduplication by canonical tool family.

Do not use learned fusion weights selected on frozen test.

## Candidate-budget frontier

Evaluate at least:

- K=16;
- K=24;
- K=32.

The previous 0.98 recall gate remains the desired target, but M005 will select the Pareto point using both recall and latency/schema/ranker cost.

Do not assume K=16 is intrinsically optimal.

## Unknown-tool/generalization requirements

The semantic path must rank by descriptor meaning rather than memorized tool IDs.

Required slices:

- synthetic renamed tools;
- plugin/MCP descriptors unseen during training;
- hard lexical mismatch where BM25 misses but semantic retrieval should recover;
- lexically obvious cases where semantic retrieval must not regress BM25 unnecessarily.

## Runtime/authority invariants

- build eligible deferred universe only after `ResolvedToolSurface`;
- hidden/denied/disabled/non-callable tools never enter cache/query set;
- cache invalidates on surface/descriptor/model hash changes;
- cached embeddings contain tool descriptor text only, not user context;
- user-context query embeddings are turn-local and not persisted unless explicitly covered by training-data consent;
- retrieval failure falls back to current deterministic BM25 path.

## Measurements

For each K and fusion mode:

- candidate Recall@K;
- relevant tools missed;
- unknown/family-holdout recall;
- preselection p50/p95 latency;
- query-encoding latency;
- cache warm/cold behavior;
- cache memory size for 16/64/128/256 tools;
- downstream ranker latency after shortlist;
- total advisor latency.

Target: >=0.98 relevant-tool recall on the existing 64-tool fixture if achievable without unreasonable local cost.

## Verification

Add regressions for:

- late-position semantic-only relevant tool;
- cache invalidation;
- denied tool absence;
- deterministic tie ordering;
- unknown MCP/plugin descriptor;
- BM25 failure / semantic recovery;
- semantic failure / BM25 union recovery;
- K frontier reporting.

## Acceptance

M004 closes positively only if hybrid retrieval materially improves candidate recall over BM25 and preserves authority/fallback behavior. If the 0.98 gate cannot be met, record the best Pareto frontier honestly for M005 rather than tuning on final test.
