# Tool-Selection Advisor Sequence-Encoder Experiment M004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-selection-advisor-sequence-encoder-experiment/005-hybrid-retrieval-shortlist-experiment.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-sequence-encoder-experiment-roadmap.md#m004--hybrid-semanticbm25-candidate-retrieval`

Repository baseline reviewed: `03190ea7f86b36c95ae459f63a20f80912443b78`

Implementation commits:

- `bf3e4b5` — advisor: implement sequence ranking retrieval qualification harness

## 1. Executive finding

M004 closes as a completed retrieval experiment with a negative material-gain
result on the current C001 corpus. The retriever evaluates the complete
deferred universe, caches only descriptor embeddings, computes a turn-local
query embedding, and supports BM25, semantic cosine, reciprocal-rank fusion,
and normalized-score union. On 256 C001 cases and 181 relevant deferred-tool
labels, both BM25 and RRF recovered 181/181 at K=16, 24, and 32. The hybrid
path therefore did not improve recall over BM25 on this corpus; that result is
carried honestly into M005 rather than being promoted as a deployment gain.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Complete authorized deferred universe | `eligible` filter and BM25 completion logic | pass | All deferred descriptors remain candidates, including zero lexical-score ties. |
| Semantic descriptor cache key | `RetrievalCacheKey` | pass | Surface, canonical name, normalized descriptor hash, encoder/tokenizer version. |
| No context persistence | in-memory `DescriptorEmbeddingCache`; report `cache_contains_context=false` | pass | Query vectors are not inserted into the cache. |
| Deterministic fusion | RRF/normalized union sorting and tie test | pass | Name is the final tie-breaker. |
| K frontier | actual `sequence-encoder-retrieve` run | pass | K=16/24/32 recorded for BM25 and RRF. |
| Fallback | `retrieve_with_fallback` and negative-path structure | pass | Encoder errors return deterministic BM25 results. |
| Authority filtering | deferred-only retrieval plus authority regression test | pass | Core/hidden/non-deferred descriptors never enter retrieval output. |
| Material recall improvement | C001 256-case measurement | not met | RRF equals BM25 at 1.000 recall; no false positive gain claim. |

## 3. Production implementation evidence

The retriever is experiment-gated and not wired into live disclosure. It
accepts a `ToolAdvisorCase` representing an already-authorized surface and
cannot add descriptors. Surface changes clear the in-memory cache. The
semantic encoder failure path is bounded and returns the deterministic BM25
ordering. Retrieval reports expose cache size, query time, total time, K,
candidate counts, and fallback state.

## 4. Verification executed

```bash
rtk cargo run --locked --features tool-advisor-encoder-experiment --bin codegg -- tool-advisor sequence-encoder-retrieve --manifest target/tool-advisor/reference-assets/all-minilm-l6-v2/manifest.json --mode rrf --k 16,24,32 --json
rtk cargo run --locked --features tool-advisor-encoder-experiment --bin codegg -- tool-advisor sequence-encoder-retrieve --manifest target/tool-advisor/reference-assets/all-minilm-l6-v2/manifest.json --mode bm25 --k 16,24,32 --json
rtk cargo test --locked --features tool-advisor-encoder-training -p codegg --lib sequence_retrieval::tests
rtk cargo fmt --all -- --check
rtk git diff --check
```

Results:

- BM25 recall was 1.000 at K=16/24/32; RRF recall was also 1.000 at all
  three K values.
- RRF mean per-case latency was 213.7ms (K=16), 196.3ms (K=24), and
  203.0ms (K=32), with a warm descriptor cache of 26 entries and no cached
  context. BM25 stayed below 1ms per case after model load.
- Retrieval unit tests passed for cache invalidation, authority filtering,
  and deterministic fusion ties. Feature check, formatting, and diff checks
  passed.

## 5. Invariant review

- Only deferred, already-authorized candidates are retrievable.
- Hidden, denied, disabled, and non-callable tools have no path into the
  experiment input; live authority remains outside this module.
- Surface/model/tokenizer changes invalidate descriptor cache entries.
- User context is turn-local and never serialized or cached.
- Retrieval failure cannot remove the deterministic BM25 fallback.

## 6. Failure and recovery review

Empty eligible universes return an empty bounded result. Invalid K fails
closed. Semantic load/encoding failures use BM25 fallback. Surface changes
clear stale embeddings. Deterministic tie ordering prevents cache or map
iteration nondeterminism. No durable state, restart migration, or concurrent
writer was introduced.

## 7. Migration and compatibility review

The retriever is additive and experiment-only. The existing BM25 path is
unchanged and remains the fallback. Cache state is ephemeral, so no migration
or compatibility format is required.

## 8. Security review

The module receives no authority beyond the caller's deferred descriptor set,
does not persist user context, and has no network or asset-download path.
Descriptor text is bounded by the existing candidate contract. Retrieval
cannot promote core or denied tools.

## 9. Documentation and operations

`architecture/tool-advisor-framework-spike.md` documents the cache and
fallback boundary. The retrieval CLI reports the K frontier and cache/failure
state in JSON for the M005 frozen protocol.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Hybrid RRF did not materially improve the current C001 corpus over BM25 | No evidence yet supports paying semantic-query cost for live promotion | M005 must evaluate expanded 64/128-tool fixtures and retain a negative disposition if the gate still fails. |
| low | Semantic query latency is roughly 200ms per case on the development Mac CPU | Research-only cost; not live behavior | Include in M005 resource report and do not wire live runtime. |

The medium finding is an explicit experiment outcome, not an unowned defect;
M005 owns the final qualification decision.

## 11. Roadmap disposition

M004 is closed as an honest hybrid-retrieval experiment. M005 is unblocked:
all of P001, M002, M003, and M004 are now closed. The existing live-primary
advisor M004 remains blocked pending M005's final qualification disposition.

## 12. Registry updates

- M004 moves from dependency-ready `ready` to recently closed with this
  record; the implementation is in `bf3e4b5`.
- M005 moves from blocked to dependency-ready `ready` in the same status
  transition because all hard dependencies are now closed.
- The M004 material-gain gate is carried as a named M005 decision input; no
  corrective plan is registered because authority, cache, and fallback
  correctness passed.
