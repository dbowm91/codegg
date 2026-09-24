# Tool-Selection Advisor Retrieval-Signal Experiment M001 — Signal Sufficiency Audit and Preregistration

Status: ready for handoff

Repository baseline: `1093ad0e3285e8ee66684e8a7f3401a200c0596e`

Source roadmap:

- `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md#m001--signal-sufficiency-audit-and-preregistration`

Controlling architecture:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Primary class: evidence/infrastructure.

## 1. Objective

Determine why the persistent retrieval misses are invisible and freeze the next experiment before changing scoring.

M001 must answer two different questions:

1. Is the needed tool inferable from the bounded retrieval query?
2. If yes, what representation signal is absent or misaligned?

No retriever/model training occurs in M001.

## 2. Persistent-miss audit

Reproduce the M003 retrieval-architecture miss set from repository-owned dev fixtures.

For every missed relevant occurrence of:

- `glob`;
- `table_filter`;
- `write`;
- `lsp_rename`;

record:

- case id;
- partition;
- relevance grade;
- preferred-order position;
- whether it is primary/highest-grade or secondary;
- serialized `AdvisorContextV2`;
- candidate name/description/category/disclosure;
- available live parameter schema fields when the candidate maps to a built-in tool;
- BM25 rank/score;
- semantic rank/cosine;
- best fused rank;
- lexical overlap terms;
- nearest wrong candidates;
- inferability classification.

Do not use v3 for classification/tuning.

## 3. Inferability taxonomy

Classify every miss as exactly one primary cause:

- **query-explicit** — the required operation is directly expressed in current objective/task/next step;
- **query-paraphrase** — operation is expressed by a semantically equivalent phrase not present in descriptor text;
- **descriptor-incomplete** — schema/operation information exists in the tool contract but is absent from retrieval text;
- **query-projection-loss** — required cue exists in available current state but current serialized/query construction drops or dilutes it;
- **implicit-secondary** — relevance label represents a later/supporting workflow step that is not inferable from the allowed query state;
- **other-evidence-defect** — label/candidate/fixture mismatch.

The classification receipt must include a rationale and the exact allowed text supporting inferability.

## 4. Hard stop on evaluation defects

If any gate-critical relevant label is classified `implicit-secondary` or `other-evidence-defect` and materially affects the 0.99/0.98/0.95 frontier:

- do not alter the gate in this workstream;
- do not train the projection to memorize it;
- close M001 blocked;
- register a separate retrieval-evaluation corrective defining the intended relevance target.

The corrective must decide whether retrieval is meant to recover all plausible workflow tools, only current-step tools, or graded recall. This experiment cannot make that product/evaluation decision implicitly.

## 5. Retrieval Signal V2 representation contract

If evidence is valid, freeze a versioned representation contract.

### Candidate descriptor fields

Allowed static fields:

- canonical name;
- identifier tokens split on snake_case, kebab-case, punctuation, and camel boundaries;
- description;
- category;
- disclosure;
- bounded parameter property names;
- bounded parameter types;
- bounded parameter descriptions;
- required/optional marker.

Parameter schema extraction must:

- cap total bytes;
- sort fields deterministically;
- exclude examples/default values that may contain secrets;
- exclude endpoints, credentials, or runtime values;
- gracefully support candidates without schemas.

Do not add hand-authored aliases keyed to `glob`, `write`, `table_filter`, or `lsp_rename`.

### Query fields

Allowed:

- `AdvisorContextV2.current_objective`;
- `current_task`;
- `next_steps`;
- `unresolved_signal`;
- `capability_cue`.

Freeze field order, byte caps, and weighting strategy.

### Generic normalization

Preregister any generic normalization before M002. Allowed examples:

- identifier decomposition;
- lowercase Unicode normalization;
- punctuation separation;
- conservative morphology shared across all tools.

A synonym/capability lexicon is permitted only if generic, small, preregistered, and tested on unseen tool names. No post-hoc additions after inspecting M002 misses.

## 6. Schema plumbing audit

Trace whether live `ResolvedToolSurface` definitions expose input schema at the candidate-construction seam.

Document:

- source type/path;
- whether `ToolAdvisorCandidate` needs an optional backward-compatible field;
- whether experiment-local `RetrievalDescriptorV2` can avoid changing the base candidate artifact schema;
- offline benchmark mapping for built-in versus synthetic candidates.

Prefer an experiment-local representation over expanding durable advisor artifacts unless runtime reuse requires otherwise.

## 7. Learned-projection preregistration

M001 also freezes the conditional M003 search space.

Base encoder:

- existing pinned MiniLM only;
- frozen transformer weights.

Projection families:

- shared linear 384→128;
- asymmetric linear query/descriptor 384→128;
- asymmetric 2-layer MLP 384→128→128.

Maximum trainable projection parameters: 500,000.

Predeclare:

- learning-rate grid;
- epoch bound;
- temperature grid;
- loss weights;
- random seeds;
- hard-negative count;
- batch size.

No expansion after M002/v3 inspection.

## 8. Training target contract

For conditional M003, positives come only from clean train relevance labels that passed M001 inferability rules.

Use graded relevance:

- grade 3 strongest positive;
- grade 2 positive with lower weight;
- grade 1 weak positive if present;
- no-tool contributes no positive descriptor.

Hard negatives come from:

- current BM25/semantic nearest wrong candidates on **train** only;
- same-family distractors;
- renamed/unknown synthetic identities from train-only generators.

Dev/test/v2/v3 never mine training negatives.

## 9. Frozen dev gates

Preserve the existing retrieval gates:

- universe 64 recall >=0.99;
- universe 128 recall >=0.98;
- universe 256 recall >=0.95;
- K<=32 for primary selection;
- zero authority violations.

No gate relaxation in M001-M004.

## 10. Outputs

Commit a compact preregistration/diagnostic receipt, e.g.:

- `assets/tool-advisor/retrieval-signal-m001-preregistration.json`

It should contain:

- miss classifications;
- representation schema/version;
- descriptor/query field contract;
- schema byte caps;
- deterministic variant grid;
- conditional learned grid;
- dataset/dev fingerprints;
- frozen gates;
- stop-condition result.

Do not commit large embeddings.

## 11. Regression tests

At minimum:

- identifier splitting deterministic;
- schema extraction excludes example/default/runtime values;
- descriptor field order stable;
- candidate without schema remains valid;
- query serializer does not include transcript/tool results/secrets;
- miss audit reproduces the four named persistent misses;
- inferability classification must include supporting text;
- preregistration hash changes on any grid/field change;
- v3 is absent from selection inputs.

## 12. Verification

```bash
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor
python3 scripts/check_execution_ownership.py
scripts/verify.sh quick
cargo fmt --all -- --check
git diff --check
```

## 13. Acceptance

Positive M001 closure requires:

- all gate-critical labels judged inferable from allowed state;
- signal gap attributed case-by-case;
- Representation V2 and conditional projection grid frozen;
- no model trained;
- no gate changed.

If inferability fails, M002 remains blocked and a separate evaluation corrective must be registered.
