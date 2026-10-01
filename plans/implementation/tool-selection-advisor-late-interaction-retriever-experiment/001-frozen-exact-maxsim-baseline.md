# Tool-Selection Advisor Late-Interaction Retriever M001 — Frozen Exact MaxSim Baseline

Status: blocked on retrieval-signal closure corrective C001

Repository baseline: `c4cc6c5b3c9154428b8560a60aa0129624435dfe`

Source roadmap:

- `plans/subsystems/tool-selection-advisor-late-interaction-retriever-experiment-roadmap.md#m001--frozen-exact-late-interaction-baseline`

Hard dependency:

- `plans/closure/tool-selection-advisor-retrieval-signal-closure-corrective/001-status.md` with repository-clean terminal closure and trustworthy hosted CI.

Primary class: architecture experiment.

## 1. Objective

Test the late-interaction hypothesis with **zero learned retrieval parameters**.

M001 answers one question:

> Does preserving MiniLM token-level representations recover useful query↔tool signal that is lost by mean pooling?

If not, the workstream stops before M002.

## 2. Encoder API

Add a narrow token-level inference API to the existing `CandleBertSequenceEncoder`.

Preferred shape:

```rust
pub struct TokenEmbeddingSequence {
    pub token_ids: Vec<u32>,
    pub vectors: Vec<Vec<f32>>,
}

pub fn encode_token_sequence(
    &self,
    text: &str,
    max_tokens: usize,
) -> Result<TokenEmbeddingSequence>
```

Equivalent typed APIs are allowed.

Requirements:

- use the same pinned MiniLM weights/tokenizer;
- one encoder sequence, not paired cross-encoding;
- exclude `[CLS]`, `[SEP]`, padding from returned scoring tokens;
- return token ids aligned one-to-one with vectors;
- no token strings/runtime text persisted in descriptor cache;
- deterministic on CPU within documented floating-point tolerance.

Do not expose arbitrary internal Candle tensors outside the advisor module.

## 3. Preregistration before dev measurement

Use a two-commit discipline.

### Commit A — architecture/preregistration

Before measuring dev retrieval, freeze:

- query token cap;
- descriptor token cap;
- truncation side/policy;
- special-token filtering;
- vector normalization epsilon;
- MaxSim aggregation;
- candidate scoring tie-break;
- exact modes to measure;
- cache key/version;
- resource limits;
- architecture-signal gates.

Token caps may be selected from **train-only/static descriptor-length evidence**, not dev retrieval outcomes. Each cap must be <=128 non-special tokens unless a separate justification is recorded.

Commit a receipt such as:

- `assets/tool-advisor/late-interaction-m001-preregistration.json`

### Commit B — measurement

Run the frozen dev frontier once and commit the result.

No cap/scoring/fusion changes after seeing dev metrics.

## 4. Primary exact MaxSim scorer

For query vectors `q_i` and descriptor vectors `d_j`:

1. L2-normalize every token vector independently.
2. For each query token, compute the maximum dot/cosine against every descriptor token.
3. Score the descriptor by the arithmetic mean of those per-query-token maxima.

Formally:

```text
score(Q,D) = (1 / |Q|) * sum_i max_j dot(norm(q_i), norm(d_j))
```

Empty post-filter token sequences fail closed for that candidate/query.

Use f32 unless the existing encoder path requires otherwise.

## 5. Retrieval texts

Use the frozen corrected retrieval target and Signal V2 text contract:

- query: `RetrievalQueryV2` flat text;
- descriptor: `RetrievalDescriptorV2` flat text.

Do not add aliases, new fields, LLM expansions, examples, defaults, or runtime values.

Bind:

- historical corpus fingerprint;
- derived retrieval relevance fingerprint `23edef17…`;
- Signal V2 schema/version;
- MiniLM manifest/tokenizer/weights hashes.

## 6. Predeclared modes

M001 may measure exactly:

1. `exact-maxsim-v1`;
2. one fixed lexical+MaxSim normalized union using the existing field-weighted Signal V2 lexical score and alpha=0.5.

No alpha sweep or RRF sweep.

Also report existing frozen baselines:

- field-weighted Signal V2 lexical;
- M002 normalized pooled union (51/53).

Baselines are not re-tuned.

## 7. Dev frontier

Measure:

- universes 64, 128, 256;
- K 16, 24, 32;
- corrected inferable relevance only;
- zero authority widening.

Report:

- recovered/eligible recall;
- per-tool misses;
- rank of the two M002 residual misses;
- current-step vs explicit-next-step;
- unknown/renamed;
- canonical-name masked;
- family slices;
- schema-present/schema-absent where available;
- query/descriptor token-count distributions.

## 8. Architecture-signal disposition

### A — directly qualifies retrieval

If a preregistered mode clears:

- u64 >=0.99;
- u128 >=0.98;
- u256 >=0.95;
- K<=32;
- 0 authority violations;
- unknown/name-masked guards below pass;

freeze that M001 retrieval candidate and make M004 ready. M002/M003 are skipped.

### B — useful late-interaction signal

M002 may become ready only if all are true:

- no correctness/authority defect;
- best M001 mode reaches at least 51/53 aggregate recall at u64/u128;
- it either reaches >=52/53 at u64/u128 **or** moves at least one M002 residual miss into K16 without introducing more than one new inferable miss;
- unknown/renamed recall regresses <=0.02 from the strongest deterministic non-learned baseline;
- canonical-name-masked recall is not below the corresponding lexical baseline by >0.02;
- no required tool-family slice regresses >0.03.

### D — no architectural signal

If B is not met, close the late-interaction workstream negative. Do not train token projections.

## 9. Resources

At universe 256 report:

- raw descriptor token-vector cache bytes;
- cache entries;
- descriptor precompute time;
- query encoder p50/p95;
- MaxSim arithmetic p50/p95;
- total retrieval p50/p95/max;
- process RSS;
- cold load;
- encoder forwards per query.

Hard resource guards for M001 architecture viability:

- descriptor cache <=64 MiB at universe 256;
- total warm retrieval p95 <=1.0 s on the recorded reference host;
- exactly one query encoder forward per turn;
- descriptor encoder work cacheable across unchanged surfaces.

A resource failure closes B even if recall improves.

## 10. Tests

At minimum:

- token/vector alignment;
- special tokens excluded;
- deterministic truncation;
- exact MaxSim hand-worked fixture;
- score invariant to descriptor token order;
- candidate presentation order cannot alter mapped score;
- cache key changes on descriptor/schema/encoder hash changes;
- cache contains no query/context data;
- authority filtering precedes scoring;
- frozen receipt/prereg fingerprint.

## 11. Verification

```bash
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
scripts/verify.sh quick
cargo fmt --all -- --check
git diff --check
```

Record hosted default-feature CI separately.

## 12. Acceptance

M001 closes with:

- frozen preregistration commit;
- one frozen dev result;
- A/B/D disposition;
- no post-result architecture tuning.

Only A opens M004 directly. Only B opens M002.
