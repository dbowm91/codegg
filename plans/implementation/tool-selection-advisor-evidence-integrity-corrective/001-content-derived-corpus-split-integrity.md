# Tool-Selection Advisor Evidence Corrective C001 — Content-Derived Corpus and Split Integrity

Status: ready for handoff

Repository baseline: `71460c0cb1421f33a62d57123ac562c8a7c4bf1c`

Source roadmap:

- `plans/subsystems/tool-selection-advisor-evidence-integrity-corrective-addendum.md#c001--content-derived-corpus-and-split-integrity`

Predecessor closure:

- `plans/closure/tool-selection-advisor-post-closure-corrective/001-status.md`

Controlling ADR:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Primary class: invariant/data correctness.

## 1. Objective

Make training/dev/test and family-holdout evidence genuinely independent of duplicated/generated payloads.

The existing 256-case corpus size may be retained or expanded, but group count alone is no longer accepted as leakage protection. Split identity must be derived from content/template lineage that cannot be bypassed by changing `semantic_group` strings.

## 2. Discovered defect

At the baseline:

- 256 cases declare 128 semantic groups;
- there are only 40 unique context strings and 30 unique candidate descriptions;
- 42 exact input+label patterns appear in more than one train/dev/test partition;
- 200/256 cases participate in those cross-split exact repeats;
- repeated examples such as identical no-tool filesystem/search/git prompts have different semantic-group IDs and therefore hash into different partitions;
- `validate_qualification_corpus()` checks floors/counterfactual counts but not content-derived cross-split duplication;
- the reported tool-family holdout is a fingerprinted subset, not a first-class partition guaranteed to be excluded from optimizer/calibration use.

The prior verification missed this because it trusted declared semantic groups and aggregate counts.

## 3. Invariants

- Exact input-equivalent examples cannot cross train/dev/test regardless of IDs.
- Generated template siblings cannot cross partitions.
- Counterfactual pairs remain in one leakage family while retaining distinct labels/tasks inside that family.
- Same-input contradictory labels are rejected unless explicitly represented as a valid counterfactual dimension with differing context/state.
- Final test data is frozen before C002 tuning.
- Tool-family holdout data is excluded from training and development calibration.
- Unknown-tool holdouts are derived only from held-out source cases or otherwise carry a partition that is never used for training.
- No private user repository content is required.
- Dataset generation remains local and deterministic.

## 4. Target data contract

Introduce an explicit leakage identity instead of relying solely on human/generated `semantic_group`.

Preferred case metadata:

- `semantic_group` — semantic task grouping;
- `generated_variant_family` — generation/template lineage;
- `leakage_group` (new schema field or equivalent derived value) — authoritative split unit;
- `input_signature` — deterministic hash of normalized model-visible input;
- `label_signature` — diagnostic hash of relevance/none labels;
- `partition` only if materialized from a deterministic partition manifest rather than caller authority.

The leakage group should include all cases sharing any of:

- exact normalized context + normalized candidate descriptor set;
- the same generator/template lineage;
- declared counterfactual family;
- explicit manually assigned leakage family.

If these relations join two groups, treat them as one connected leakage component before partitioning.

Do not include relevance labels in `input_signature`; identical inputs with conflicting labels should be visible as a validation problem rather than hidden by distinct hashes.

## 5. Normalization/signature rules

Build deterministic canonicalization for fixture validation only; do not change runtime tokenizer semantics.

At minimum normalize:

- Unicode and case consistently;
- whitespace;
- candidate ordering for set-equivalent candidate sets;
- candidate descriptor fields used by the advisor: name where identity matters, description, category, disclosure;
- known generated synthetic names via stable synthetic identity metadata so renamed unknown-tool cases can be grouped correctly when appropriate.

Produce both exact and normalized duplicate reports.

Near-duplicate/template protection should use generator/template lineage, not heuristic fuzzy matching as the sole gate.

## 6. Partition model

Create an explicit dataset partition report with:

- train;
- dev/calibration;
- final test;
- one or more true tool-family holdouts.

Use connected leakage groups as the only split units.

For tool-family holdouts, support at least four rotating/reportable families from the existing high-value set (for example plugin, LSP, research, structured). A family-holdout evaluation run MUST build training/dev data excluding that family rather than merely compute a fingerprint over examples that also appeared in training.

Unknown-tool evaluation must similarly identify its source partition and prove no transformed case entered optimizer/calibration input.

## 7. Dataset quality floors

Retain the current breadth floors but add uniqueness/evidence floors:

- >=256 total cases;
- >=128 leakage groups or document a smaller number justified by real unique tasks;
- >=192 unique normalized model-visible input signatures, unless closure demonstrates a stronger non-template diversity metric;
- zero exact input-signature overlap across train/dev/test;
- zero generator/template family overlap across train/dev/test;
- >=40 unique final-test leakage groups;
- >=32 no-tool cases;
- >=32 multi-tool cases;
- >=64 hard-negative cases;
- >=32 unknown-tool holdout cases;
- >=32 valid contextual counterfactual pairs;
- >=10 task families;
- >=4 true tool-family holdout evaluations.

Case count cannot be increased by duplicating a payload under a new group identifier.

## 8. Production changes

Expected areas:

- `src/tool_advisor/mod.rs` case schema, validation, coverage report, split construction;
- local corpus-generation helper/script if generation is repository-owned;
- `assets/tool-advisor/corpus.jsonl` regenerated fixtures;
- CLI lint/report output;
- training partition APIs consumed by C002.

If a schema version is incremented, preserve bounded compatibility for old fixtures or fail with a clear migration message. Runtime model artifacts are not changed in C001.

## 9. Static/regression guards

Add tests that intentionally create:

1. byte-identical examples with different semantic-group IDs — lint must fail or co-locate them;
2. normalized-equivalent whitespace/case variants — cannot cross partitions;
3. same template lineage under different IDs — cannot cross partitions;
4. exact same input with contradictory relevance — must fail validation unless the context actually differs;
5. counterfactual pair — remains one leakage group;
6. held-out tool family — zero examples from that family in optimizer/calibration sets;
7. unknown-name transform — source remains held out.

Add a machine-readable leakage report to `tool-advisor lint --json`.

## 10. Ordered work

A. Add leakage signatures/group construction and schema compatibility.
B. Add duplicate/template/contradiction lint.
C. Implement first-class partition manifest/report and family holdout semantics.
D. Regenerate/review corpus to meet uniqueness floors.
E. Freeze train/dev/test and family-holdout fingerprints.
F. Rerun BM25/keyword/linear baselines only as descriptive evidence; do not claim contextual-model qualification yet.
G. Update architecture documentation.

## 11. Verification

Expected minimum:

```bash
cargo test --locked -p codegg --lib tool_advisor
cargo test --locked --features tool-advisor-training -p codegg --lib tool_advisor
cargo run --locked --bin codegg -- tool-advisor lint --dataset assets/tool-advisor/corpus.jsonl --json
cargo run --locked --bin codegg -- tool-advisor bench --dataset assets/tool-advisor/corpus.jsonl --json
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

Closure must include a separate audit demonstrating zero cross-partition exact/normalized input signatures.

## 12. Acceptance criteria

C001 closes only when the frozen corpus can no longer reproduce the baseline leakage defect, true family holdouts are optimizer-excluded, unique-input floors are met, and all partition fingerprints are recorded for C002/C004.

## 13. Stop conditions

Stop if the proposed fix merely renames semantic groups, hashes case IDs differently, or moves duplicate payloads around without content/template-derived grouping.

## 14. Closure evidence

- total/unique context/input/candidate counts;
- leakage-component count;
- exact and normalized cross-split overlap = zero;
- template-lineage overlap = zero;
- partition and family-holdout fingerprints;
- family exclusion matrix;
- counterfactual and unknown-tool counts;
- exact verification output.
