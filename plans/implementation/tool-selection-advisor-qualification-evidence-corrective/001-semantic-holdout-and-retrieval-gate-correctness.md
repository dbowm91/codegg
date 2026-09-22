# Tool-Selection Advisor Qualification Evidence Corrective C001 — Semantic Holdout and Retrieval-Gate Correctness

Status: ready for handoff

Repository baseline: `739bf5060690fa71dffd25ffeb1b28e444a00681`

Source corrective roadmap:

- `plans/subsystems/tool-selection-advisor-qualification-evidence-corrective-addendum.md#c001--semantic-holdout-and-retrieval-gate-correctness`

Predecessor closure:

- `plans/closure/tool-selection-advisor-sequence-qualification-corrective/002-status.md`

Controlling ADR:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Primary class: correctness/evidence corrective.

## 1. Objective

Fix the two concrete defects discovered after v2 closure:

1. make retrieval evidence identify the **candidate universe size** separately from shortlist `k`;
2. replace index-tagged synthetic qualification examples with a new, behaviorally valid v3 holdout.

C001 does not run the selected model on v3.

## 2. Immutable inputs

The following remain frozen:

- selected sequence artifact SHA-256:
  `01b5c368b4e762dbe6ca284694b290b72e17ec64606e1f26a21510721f9a10e8`;
- MiniLM encoder/config/tokenizer/source-weight hashes from v2;
- architecture `sequence-encoder-packed-marker-v1`;
- mean pooling;
- dev-selected abstention threshold;
- linear baseline artifact;
- historical C001 corpus;
- historical v2 holdout/result.

Any production-model change stops this corrective and requires a new experiment.

## 3. Retrieval frontier schema correction

Extend the qualification retrieval point with explicit fields such as:

- `candidate_universe_size`;
- `shortlist_k`;
- `eligible_relevant_tools`;
- `recovered_relevant_tools`;
- `recall`.

Do not overload `k`.

For a 64-tool fixture with K=16 the evidence must state:

```text
candidate_universe_size = 64
shortlist_k = 16
```

The gate selects points by:

- expected universe size 64 or 128;
- expected shortlist K from preregistration;
- expected retrieval mode.

It MUST NOT search for `k == 64` or `k == 128`.

Preserve old v1/v2 result deserialization where useful for historical artifacts, but qualification-v3 uses the corrected schema.

## 4. Retrieval fixture invariants

Before measuring recall, validate for every non-no-tool fixture case:

- at least one labeled relevant tool exists in the candidate universe;
- the relevant descriptor survives authority/deferred eligibility construction;
- candidate names are unique after canonicalization;
- the requested universe size is actually reached;
- distractor expansion does not replace/remove relevant tools;
- `shortlist_k <= candidate_universe_size`.

Fail closed instead of reporting zero when fixture construction is invalid.

Add a small known-answer retrieval regression:

- semantic/BM25 obvious relevant tool;
- universe 64;
- K=16;
- known relevant tool must be present;
- measured recall >0;
- result point must report universe 64/K16.

This specifically prevents recurrence of the historical zero-by-lookup bug.

## 5. New v3 holdout

Do not edit, relabel, or reuse the observed v2 cases.

Create new files, for example:

- `assets/tool-advisor/qualification-v3-holdout.jsonl`;
- `assets/tool-advisor/qualification-v3-holdout-manifest.json`;
- a v3 generator/source-of-truth under `scripts/`.

Target:

- >=160 cases;
- >=120 semantic/leakage groups;
- >=64 distinct scenario templates or manually authored scenario skeletons;
- >=24 no-tool;
- >=24 true counterfactual cases comprising >=12 explicit pairs;
- >=24 actual unknown/renamed cases;
- >=24 hard-negative;
- >=24 multi-tool;
- >=24 AdvisorContextV2/long-session cases;
- >=16 cases each for plugin/MCP, LSP, research/search, structured/data;
- include ordinary code/file/git/shell-adjacent hard negatives so the set is not dominated by four specialist families.

Cases may satisfy multiple valid slices, but tags must follow semantic construction rather than index arithmetic.

## 6. No-tool semantics

A no-tool case MUST NOT explicitly ask to use one of its candidates.

Valid examples include:

- conversational/request-for-explanation prompts requiring no tool;
- task state where all supplied deferred tools are irrelevant;
- task already completed/current state says no action is required;
- user asks for reasoning based entirely on already-present context.

Validator requirements:

- relevance/preferred order empty;
- `none=true`;
- no exact candidate name in an imperative "use/call/run/open" construction;
- no generated template field declaring a required tool;
- human-readable rationale states why abstention is correct.

Do not rely only on string lint; generator/source data must encode scenario kind `no_tool`.

## 7. True counterfactuals

Counterfactual evidence is pair-based.

Each pair shares most wording/state/candidates but changes one material semantic cue such that the correct label changes.

Examples:

- "find the literal occurrence" → grep versus "jump to the symbol definition" → LSP definition;
- "search repository docs" → local/repo search versus "search current web documentation" → web search;
- "inspect JSON record field" → structured query versus "search textual JSON source" → grep/read.

Each pair records:

- `counterfactual_pair_id`;
- changed cue;
- expected label A/B;
- shared candidate universe.

Validator must prove both pair members exist and their preferred/relevance labels differ.

Do not tag no-tool cases as counterfactual unless they belong to a real paired semantic change.

## 8. Actual unknown/renamed tools

Unknown/renamed cases must change identity, not merely add a tag.

Use synthetic but realistic canonical names such as:

- `mcp__novel_lsp__jump_target`;
- `plugin__atlas__catalog_lookup`;
- `tool_x17`.

Requirements:

- name absent from all historical train/dev/test candidate names;
- semantic description remains sufficient to infer purpose;
- `synthetic_identity=true`;
- label refers to the renamed candidate;
- include lexical-name-obscured and meaningful-new-name variants.

Validator cross-checks names against all historical corpora.

## 9. AdvisorContextV2 / long-session scenarios

Construct actual bounded state-shaped contexts, for example:

```text
Current objective: repair the failing Rust build.
Current task: locate the definition of ConfigLoader.
Next step: inspect symbol definition before editing.
Unresolved signal: compiler reports an unknown field in ConfigLoader.
Original session topic: document the architecture.
```

Requirements:

- current objective/task conflicts with or materially supersedes a stale original topic in at least half the slice;
- fields match the AdvisorContextV2 serializer vocabulary/schema closely enough to exercise production representation;
- label follows current task, not stale origin;
- include unresolved-error and no-error variants.

A short ordinary prompt with only a `context-v2-long-session` tag fails validation.

## 10. Hard negatives and multi-tool semantics

Hard negative:

- at least two plausible candidates;
- relevant candidate cannot be selected solely from exact tool-name repetition in context;
- rationale identifies why the distractor is plausible but wrong.

Multi-tool:

- at least two genuinely relevant tools;
- graded relevance/preferred order reflects task sequence or utility;
- not merely a fallback tool auto-labeled relevant.

## 11. Semantic diversity controls

Unique IDs are not semantic diversity.

Manifest reports at least:

- distinct scenario kinds;
- distinct scenario templates/skeletons;
- distinct normalized contexts after removing unique record tokens;
- candidate-name sets;
- descriptor sets;
- family counts;
- slice counts.

Minimum:

- >=64 normalized scenario skeletons;
- no single skeleton >5% of cases;
- no single family >25%;
- at least 20% of non-no-tool cases where the relevant candidate's exact name does not appear in context.

A generator may use templates, but template/skeleton identity must be explicit and enforced.

## 12. Leakage

Compare v3 against:

- historical C001 train/dev/test;
- historical family holdouts;
- observed v2 holdout.

Require zero:

- exact normalized input overlap;
- template/skeleton lineage overlap;
- explicit leakage-family overlap;
- copied context overlap.

Unknown renamed candidate names also must be novel versus all prior corpora.

## 13. Construction isolation

The v3 generator/authoring validator:

- has no sequence-model inference;
- has no linear/BM25 scoring used to choose or alter labels;
- has no remote teacher/provider call;
- may run deterministic structural/semantic lint only.

C001 MUST NOT create any selected-model v3 predictions/results.

## 14. Regression tests

At minimum:

- 64-universe/K16 point is selected correctly by gate;
- 128-universe/K16 point is selected correctly;
- absent universe point fails closed, not `unwrap_or(0.0)`;
- relevant tool missing from expanded fixture is an error;
- contradictory no-tool imperative case is rejected;
- fake unknown tag without actual novel identity is rejected;
- orphan counterfactual case rejected;
- counterfactual pair with same label rejected;
- long-session tag without AdvisorContextV2-shaped fields rejected;
- repeated skeleton floor violation rejected;
- v2 holdout overlaps rejected;
- selected artifact hash remains frozen.

## 15. Verification

```bash
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib tool_advisor::sequence_qualification
cargo test --workspace --locked -- --test-threads=1
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

Do not run `sequence-encoder-qualify-v3` against the new holdout during C001.

## 16. Acceptance

C001 closes only when:

- retrieval universe/K semantics are corrected and regression-tested;
- v3 meets all semantic floors and validators;
- v3 proves zero leakage into historical and v2 evidence;
- selected model is not evaluated on v3;
- no model/runtime behavior changed.

Positive C001 closure makes C002 ready.
