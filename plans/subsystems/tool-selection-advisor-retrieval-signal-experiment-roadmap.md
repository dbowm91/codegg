# Tool-Selection Advisor Retrieval-Signal Experiment Roadmap

Status: active

Repository planning baseline: `1093ad0e3285e8ee66684e8a7f3401a200c0596e`

Controlling architecture:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Predecessor evidence:

- `plans/closure/tool-selection-advisor-order-invariance-experiment/003-status.md` — selected span-pooled order-robust ranker.
- `plans/closure/tool-selection-advisor-order-invariance-experiment/004-status.md` — no K<=32 operating point; best recall 0.9444.
- `plans/closure/tool-selection-advisor-retrieval-architecture-experiment/003-status.md` — fusion/pooling/K follow-up closed negative with a complete coarse frontier and miss attribution.
- `plans/closure/tool-selection-advisor-retrieval-architecture-closure-corrective/001-status.md` — repository-clean closure restored.
- `assets/tool-advisor/order-invariance-m003-selection.json` — frozen downstream ranker selection.

## 1. Purpose

The previous work established that retrieval, not downstream ranking, is the current advisor blocker.

The selected span-pooled ranker corrected the candidate-order failure:

- dev permutation top-1 consistency 0.951;
- v3 diagnostic Recall@1 recovered from 0.0 to ~0.465;
- v3 diagnostic MRR ~0.625.

However, the coarse retriever cannot consistently place every relevant tool into the ranker's candidate pool. Across the completed retrieval-architecture frontier, the same four tools remain at or near the bottom of both lexical and semantic orderings:

- `glob`;
- `table_filter`;
- `write`;
- `lsp_rename`.

At 128/256 candidates the best coarse recall is 68/72 = 0.9444, including at K=64. Fusion cannot recover tools that neither source signal ranks into the pool.

The next experiment therefore changes **what retrieval represents and learns**, not another fusion constant.

## 2. Current signal limitations

Current lexical retrieval indexes essentially:

```text
canonical name + description
```

Current semantic retrieval embeds:

```text
canonical name + description + category + disclosure
```

against serialized `AdvisorContextV2`.

It does not currently provide the retriever with:

- parameter/property names and descriptions;
- identifier-token decomposition as a first-class field;
- bounded capability/operation cues;
- field-aware weighting;
- a retrieval-specific learned query/descriptor projection;
- a contrastive objective trained specifically for tool recall.

Observed vocabulary mismatches include:

- `glob`: "Find files by path pattern" versus contexts such as "Locate every test fixture matching ...";
- `write`: "Write content to files" versus "create", "persist", "save";
- `lsp_rename`: explicit rename cases plus lower-grade secondary workflow labels;
- `table_filter`: direct "filter tabular export" cases plus lower-grade secondary pipeline labels.

The last two make a second question mandatory: whether every historical "relevant" label is actually inferable from the permitted retrieval query, or whether some secondary labels encode hidden workflow knowledge.

## 3. Durable invariants

- Advisor remains optional, local, default-off, and advisory-only.
- `ResolvedToolSurface` remains the authority source; retrieval never adds candidates outside its eligible deferred universe.
- No model or retriever may execute a tool.
- Existing span-packed ranker artifact remains frozen throughout M001-M004.
- Historical train/dev/test/v2/v3 corpora remain immutable.
- v3 remains diagnostic-only and MUST NOT select representation, model, hyperparameters, thresholds, or K.
- No remote model, telemetry, or automatic weight download.
- Descriptor caches contain static tool metadata/embeddings only, never user context.
- If M001 finds a relevance target not inferable from allowed query state, this workstream stops and registers an evaluation-label corrective. It MUST NOT lower recall gates or train around hidden-label leakage.
- Final positive qualification requires a fresh v4 holdout.

## 4. Hypotheses

### H1 — descriptor/query representation gap

The generic MiniLM embedding is operating on sparse descriptions that omit useful schema/capability terms and uses the full serialized context without retrieval-specific field weighting.

### H2 — generic embedding geometry is insufficient

Even after deterministic representation enrichment, a generic pretrained embedding may not align CodeGG task language with tool descriptions. A small local projection trained over frozen MiniLM embeddings may be enough without another transformer.

### H3 — some historical relevance labels are not query-inferable

Low-grade secondary labels may describe a plausible later workflow step rather than a tool identifiable from the bounded current-state query. If true, retrieval evaluation must distinguish an evidence defect from a model defect before any learned projection is selected.

## 5. Target architecture

The experiment has two escalating signal layers.

### Layer A — deterministic Retrieval Signal V2

A versioned retrieval representation derived only from safe static tool metadata and bounded advisor state.

Candidate side may include:

- canonical name;
- split identifier tokens (snake/kebab/camel boundaries);
- description;
- category/disclosure;
- bounded parameter property names/types/descriptions when available;
- deterministic operation/capability terms derived by a documented generic normalizer.

Query side may include:

- field-aware `AdvisorContextV2` objective/task/next-step/unresolved/capability fields;
- identifier decomposition;
- conservative lexical normalization.

No per-tool hand-authored aliases may be added merely to repair the four known misses. Any lexical normalization must be generic and testable on unseen names/tools.

### Layer B — small frozen-encoder retrieval projection

If deterministic Signal V2 does not clear the dev recall gates, reuse the existing frozen MiniLM encoder and train small query/descriptor projection heads.

Preferred shape:

```text
MiniLM query embedding      -> query projection ----\
                                                   cosine / dot
MiniLM descriptor embedding -> descriptor projection/
```

The base transformer remains frozen. Projection parameter count should remain <=500k unless M003 preregisters a smaller justified alternative. This reuses weights already needed by the advisor and avoids another model family.

## 6. Dependency graph

```text
M001 signal sufficiency + representation preregistration
                |
                v
M002 deterministic Retrieval Signal V2
                |
        +-------+-------+
        | positive      | insufficient but valid
        v               v
M004 operating point   M003 learned projection
        ^               |
        +---------------+
                |
                v
M005 fresh v4 qualification
```

- M001 is ready.
- M002 is blocked on M001.
- M003 is conditional: only if M002 does not clear the frozen dev recall gates and M001 found no evaluation-label defect.
- M004 requires either positive M002 or positive M003.
- M005 requires positive M004.

## 7. Milestones

### M001 — Signal sufficiency audit and preregistration

Plan:

- `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/001-signal-sufficiency-audit-and-preregistration.md`

Status: ready.

Audit every persistent miss at case level, determine whether the relevance label is inferable from allowed query state, freeze Retrieval Signal V2 field/normalization boundaries, and preregister deterministic and learned experiment degrees of freedom.

### M002 — Deterministic Retrieval Signal V2

Plan:

- `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/002-deterministic-retrieval-signal-v2.md`

Status: blocked on M001.

Implement versioned field-aware query/descriptor construction and measure the same 64/128/256-tool dev frontier without training.

### M003 — Frozen-encoder learned retrieval projection

Plan:

- `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/003-frozen-encoder-retrieval-projection.md`

Status: blocked/conditional on M002.

If needed, train small asymmetric query/descriptor projection heads over frozen MiniLM embeddings using train-only graded positives and hard negatives; select on dev retrieval quality/generalization.

### M004 — Retrieval + promotion operating point

Plan:

- `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/004-retrieval-and-promotion-operating-point.md`

Status: blocked on positive M002 or M003.

Freeze the smallest/cheapest dev retrieval point that clears the existing recall gates, then attach the already-separated candidate-relevance promotion calibration.

### M005 — Fresh v4 qualification

Plan:

- `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/005-fresh-v4-preregistered-qualification.md`

Status: blocked on M004.

Build a new zero-leakage, order-balanced semantic holdout and run one separately preregistered release-mode qualification of the complete retrieval + span-packed ranker + promotion stack.

## 8. Exit conditions

Positive completion requires:

- no evaluation-label defect;
- 64-tool retrieval recall >=0.99 at bounded K;
- 128-tool recall >=0.98;
- 256-tool recall >=0.95;
- zero authority violations;
- candidate-order robustness preserved downstream;
- promotion confidence remains separate from abstention;
- fresh-v4 quality/order/retrieval/promotion/resource gates pass.

A negative result is valid closure. Live-primary-model M004 remains blocked unless M005 records disposition A.
