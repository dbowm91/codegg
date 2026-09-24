# Tool-Selection Advisor Retrieval-Signal Experiment M002 — Deterministic Retrieval Signal V2

Status: blocked on M001

Repository baseline: `1093ad0e3285e8ee66684e8a7f3401a200c0596e`

Hard dependency:

- M001 positive signal-sufficiency closure.

Source roadmap:

- `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md#m002--deterministic-retrieval-signal-v2`

Primary class: experimental infrastructure.

## 1. Objective

Implement the preregistered Retrieval Signal V2 representation without learned retrieval weights and determine whether better static signal alone clears the large-catalog dev gates.

## 2. Representation implementation

Introduce an experiment-versioned representation, e.g.:

- `RetrievalQueryV2`;
- `RetrievalDescriptorV2`;
- `RETRIEVAL_SIGNAL_SCHEMA_VERSION = 2`.

Do not silently modify historical v1 retrieval semantics.

Candidate descriptor construction must follow the exact M001 field contract. Query construction must follow the exact M001 `AdvisorContextV2` field contract.

## 3. Field-aware lexical scorer

Implement an experiment-local field-aware lexical scorer.

At minimum compare preregistered variants such as:

- canonical v1 BM25;
- descriptor-v2 flat BM25;
- field-weighted BM25/name+operation/schema;
- generic normalized-token BM25.

Weights are fixed by M001 preregistration. Do not tune new weights after measurement.

Catalog `ToolCatalog` behavior remains unchanged; this is advisor experiment code until qualification.

## 4. Semantic descriptor/query variants

Using the frozen MiniLM encoder, compare preregistered deterministic encodings:

- flat Retrieval Signal V2 text;
- field-labelled descriptor text;
- field-labelled query text;
- M001-approved pooling options.

Do not train encoder/projection weights in M002.

Descriptor embedding cache key must include:

- representation schema/version;
- descriptor fingerprint;
- encoder/tokenizer hash;
- pooling;
- surface fingerprint.

Cache contains descriptor embeddings only.

## 5. Built-in versus synthetic tools

For built-in tools with schema:

- include bounded schema-derived fields.

For plugin/MCP/synthetic candidates without a native Rust schema:

- use whatever static input schema is already present in the resolved definition when available;
- otherwise degrade to name/description/category/disclosure;
- never invent schema fields.

Report recall separately for schema-present and schema-absent candidates.

## 6. Dev frontier

Measure the same frozen dev fixtures:

- universes 64, 128, 256;
- K 16, 24, 32;
- zero authority violations.

Report:

- aggregate recall;
- per-tool recall;
- primary/highest-grade versus secondary relevance recall;
- built-in versus external/synthetic;
- schema-present versus schema-absent;
- latency p50/p95/max;
- descriptor cache size;
- first-load/warm costs.

## 7. Persistent-miss closure

For each M001 persistent miss, show before/after:

- lexical rank;
- semantic rank;
- best deterministic rank;
- whether it entered K16/K24/K32.

A positive aggregate result cannot hide a regression where an entire tool family becomes unreachable.

## 8. Positive path

If a deterministic point clears:

- 64 >=0.99;
- 128 >=0.98;
- 256 >=0.95;
- K<=32;
- zero authority violations;

freeze that representation candidate for M004. M003 learned projection is skipped/not needed.

Selection order:

1. smallest K;
2. lower p95 latency;
3. lower descriptor bytes/cache size;
4. deterministic mode name tiebreak.

## 9. Negative-but-valid path

If M002 remains below a gate but:

- labels are valid/inferable;
- persistent misses materially improve or change;
- no correctness/authority defect exists;

close M002 as insufficient deterministic signal and unblock conditional M003.

Do not expand representation fields or add synonyms after seeing results.

## 10. Stop conditions

Stop and register corrective work if:

- live and offline descriptor construction cannot be made equivalent;
- schema extraction leaks defaults/examples/runtime values;
- representation depends on candidate authority outside the allowed surface;
- M001 preregistration cannot be reproduced.

## 11. Verification

```bash
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor
scripts/verify.sh quick
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
git diff --check
```

## 12. Acceptance

M002 closes with one deterministic frontier receipt and either:

- a frozen positive retrieval representation for M004; or
- a documented negative result that makes M003 ready.

No downstream live advisor work is unblocked directly.
