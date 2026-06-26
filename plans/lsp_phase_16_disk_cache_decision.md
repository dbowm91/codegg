# Decision Record: LSP Disk Cache (Phase 16 Workstream 5)

**Decision Date:** 2026-06-26
**Status:** Accepted
**Decision:** Defer disk-backed LSP semantic cache

## Evaluation Summary

Phase 16 Workstreams 1 and 2 evaluated whether to add a disk-backed semantic cache to the LSP module (`LspSemanticCache` in `crates/egglsp/src/cache.rs`). The evaluation followed the evidence-driven approach defined in the Phase 16 disk cache evaluation plan (`plans/lsp_phase_16_disk_cache_evaluation_plan.md`). This record captures the decision and rationale.

## Benchmark Summary

| Metric | Value | Notes |
|--------|-------|-------|
| Cache key hashing | ~100-200ns | Negligible |
| Packet serialization roundtrip | ~325µs | Per entry (10 items) |
| Packet size | ~700 bytes/item | ~256KB for 64 entries (5 items avg) |
| File hash collection | ~1.7ms | Bottleneck; happens on cache miss regardless |
| Disk write | ~220µs | Per entry |
| Disk read + deserialize | ~670µs | Per entry |
| Total disk path overhead | ~890µs | Write + read roundtrip |
| Memory overhead | Near-zero | Beyond serialized data |

**Key finding:** Disk I/O is technically viable. A disk-backed cache adds ~900µs overhead versus memory-only. Performance is NOT the blocker for this decision.

### Workflow scenario benchmarks

| Workflow | Items | Serialized size | Serialize/entry | Deserialize/entry |
|----------|-------|-----------------|-----------------|-------------------|
| review-diff (5 files × 6) | 30 | 19,490 bytes | ~1.9ms | ~1.2ms |
| repair-local | 8 | 5,859 bytes | ~534µs | ~518µs |
| impact-analysis | 26 | 17,174 bytes | ~1.6ms | ~1.1ms |
| test-failure repair | 7 | 5,342 bytes | ~480µs | ~383µs |
| call-neighborhood | 11 | 7,625 bytes | ~788µs | ~520µs |

**Key finding:** Workflow packet sizes range from ~5KB to ~19KB. Serialize/deserialize costs scale linearly with item count (~60-70µs per item). These are comparable to the disk I/O overhead, reinforcing that disk persistence adds marginal benefit.

### Cache hit/miss/stale rate behavior

Under a 5-workflow repeated-access pattern (insert → hit → file-hash-change → stale-miss → generation-change → stale-miss → re-insert → hit):

- **Hit rate:** 50% (10 hits / 20 lookups)
- **Miss rate:** 50% (10 misses / 20 lookups)
- **Stale miss rate:** 100% of misses are stale (file hash or generation mismatch)

**Key finding:** Stale misses dominate. In realistic workflows, file hashes change frequently during active development and LSP server restarts invalidate all entries via generation mismatch. This limits the cold-start benefit of disk persistence.

### Startup-after-restart simulation

| Phase | Per-workflow cost | Notes |
|-------|-------------------|-------|
| Cold insert (first session) | ~2.3ms | Serialize + memory insert |
| Cold lookup (restart, all miss) | ~3.5ms | All 5 lookups miss; would require disk load + deserialize |
| Warm lookup (same session) | ~39µs | Memory hit; ~90× faster than cold miss |

**Key finding:** Warm memory hits are ~90× faster than cold misses. However, cold misses after a restart are inevitable (new cache instance) and would only be mitigated by disk persistence. The ~3.5ms cold lookup cost for 5 workflows is small in absolute terms.

## Privacy/Threat Model Summary

Two high-risk scenarios were identified (full analysis in `architecture/lsp_disk_cache_threat_model.md`):

| ID | Scenario | Likelihood | Impact |
|----|----------|------------|--------|
| T3 | Plaintext source snippets on disk | Medium | High |
| T7 | Secrets leaked from cached source code | Low | High |

**Required mitigations before any disk persistence:**
- Encryption at rest (platform-specific complexity)
- Content filtering to redact code snippets while preserving utility (unsolved problem)
- Explicit user opt-in

Content filtering—redacting sensitive code while retaining diagnostic/symbolic value—remains an unsolved design problem. Encryption adds platform-specific dependencies and complicates cross-platform builds.

## Selected Option

**Option C: No disk cache (defer)**

Memory-only cache remains the only active mode. The existing `LspSemanticCache` configuration (`[lsp_semantic_cache]` with `mode: "memory"`) continues as-is.

## Rationale

1. **Marginal performance benefit:** The ~500µs disk overhead is small, but cold-start improvement is limited because cache entries expire after 5 minutes (TTL), file hashes change frequently during active development, LSP server restarts invalidate all entries via generation mismatch, and the actual bottleneck is file hash collection (~1.6ms), not cache lookup.

2. **Significant privacy cost:** Plaintext source snippets would persist across sessions. Secrets in source code could be extracted from cache files. Encryption adds platform-specific complexity. Content filtering (redacting sensitive code) is an unsolved design problem.

3. **Substantial complexity cost:** Schema versioning, migration, and corrupt entry handling. Root isolation with normalized paths. Cross-platform cache directory management. Additional TUI commands and config options.

4. **Memory cache already provides the primary benefit:** Within-session reuse eliminates redundant LSP calls. Conservative eviction ensures correctness. Disabled by default keeps the system simple.

## Follow-up Items

1. Re-evaluate if user feedback indicates cold-start latency is a pain point.
2. Consider encrypted cache if platform crypto APIs become simpler.
3. Consider content-filtered cache (store symbols/diagnostics only, not source text).
4. Monitor cache hit rates in production to validate memory-only is sufficient.
5. Consider workspace-level cache warming on startup (pre-populate from previous session's file hashes).
