# LSP Phase 16 Plan: Optional Disk Cache Evaluation

Status date: 2026-06-26
Phase type: performance evaluation / privacy design / optional persistence
Prerequisites: Phase 15 renderer-policy diagnostics complete enough to make cache behavior observable.

## Purpose

Phase 16 should evaluate whether a disk-backed semantic cache is worth implementing. The current Phase 12 cache is memory-only, optional, bounded, conservative, and disabled by default. Disk persistence could improve startup latency and repeated-session behavior, but it adds privacy, invalidation, schema, stale-evidence, and cleanup risks.

This phase must be evidence-driven. It is acceptable, and likely preferable, for Phase 16 to conclude that disk cache should remain deferred.

## Current baseline

The repo already has:

- `LspSemanticCache` memory mode.
- Disabled-by-default config.
- Root/server/request/budget/capability/file-hash cache keys.
- Conservative eviction on TTL, file hash change, generation mismatch, and capability/config mismatch.
- Cache status and clear commands.
- Request-scoped file hashes in production cache keys.
- Context diagnostics planned in Phase 15.

## Non-goals

Do not implement disk cache before benchmarking and privacy review.

Do not make disk cache default-on.

Do not cache raw LSP protocol JSON.

Do not cache mutation-producing preview apply plans as reusable writes.

Do not store cache entries across unrelated workspace roots.

Do not persist source-derived evidence without explicit user opt-in.

Do not create unbounded cache growth.

## Workstream 1: performance benchmark design

### Questions to answer

- How often does memory cache hit in realistic agent workflows?
- Which operations are expensive enough to benefit from persistence?
- Is LSP server response time or model latency the actual bottleneck?
- Does cold-start disk cache materially improve first useful response?
- How expensive is cache key hashing across typical changed files?

### Target files

- benchmark harness if present,
- `crates/egglsp/benches/` if benchmarks are supported,
- `src/tool/lsp.rs`,
- `crates/egglsp/src/cache.rs`,
- docs.

### Benchmark scenarios

- no cache,
- memory cache cold,
- memory cache warm,
- simulated disk load/store using serialized packets,
- repeated review-diff workflow,
- repeated repair-local workflow,
- impact-analysis workflow,
- test-failure repair workflow,
- startup after process restart.

### Metrics

- context collection duration,
- cache lookup duration,
- cache insert duration,
- serialization/deserialization duration,
- file-hash collection duration,
- packet size,
- rendered output size,
- cache hit/miss rate,
- stale miss rate.

### Acceptance criteria

- There is a small reproducible benchmark or measurement harness.
- Results justify either deferring or implementing disk cache.

## Workstream 2: privacy and threat model

### Problem

Disk cache stores source-derived semantic evidence. That may include code snippets, diagnostics, symbol names, paths, comments, and security-sensitive findings.

### Required analysis

Document:

- what would be stored,
- where it would be stored,
- who can read it locally,
- how it is cleared,
- how it is scoped by workspace root,
- whether paths are absolute or normalized,
- whether source text snippets are stored,
- how schema/version mismatch is handled,
- whether secrets in source can appear in cached evidence,
- how users disable it.

### Target files

- `architecture/lsp.md`,
- `.opencode/skills/lsp/SKILL.md`,
- security docs if present,
- config docs.

### Acceptance criteria

- Disk cache threat model exists before implementation.
- Disk cache remains explicit opt-in if implemented.

## Workstream 3: storage design, only if justified

### Storage requirements

If benchmarks justify implementation, design a conservative store:

- mode: `disabled | memory | disk`, with `disabled` default.
- root-scoped directory under Codegg cache directory.
- schema version in every entry or database.
- max entries and max bytes.
- TTL.
- drop-on-version-mismatch behavior.
- manual clear all and clear root.
- root hash or normalized root ID to avoid path traversal.
- no cross-root lookup.

### Storage format options

Option A: SQLite.

Pros: robust metadata queries, easy size accounting, transactional writes.
Cons: dependency/locking and migration complexity.

Option B: directory of serialized entries.

Pros: simple, inspectable, fewer moving parts.
Cons: harder eviction and atomicity.

Option C: no disk cache.

Pros: safest.
Cons: no warm-start gains.

### Recommendation

Prefer no disk cache unless benchmark data shows strong wins. If implemented, prefer SQLite only if the repo already depends on SQLite or has a cache/storage abstraction. Otherwise use a simple directory store with atomic write-and-rename and schema-versioned files.

### Acceptance criteria

- Storage design is selected based on benchmark evidence.
- Design preserves root isolation, TTL, caps, and opt-in semantics.

## Workstream 4: implementation, only if approved

### Target files

- `crates/egglsp/src/cache.rs`,
- new disk cache module if needed,
- config schema,
- `src/tool/lsp.rs`,
- TUI cache commands,
- docs.

### Implementation steps

1. Add `LspCacheMode::Disk` only after design approval.
2. Add config fields for cache directory, max entries, max bytes, TTL, and clear-on-version-mismatch if needed.
3. Add schema version.
4. Add root-scoped key prefix.
5. Serialize canonical packets, not rendered strings.
6. Enforce TTL and caps on read/write.
7. Never return a stale entry as fresh.
8. Add clear root and clear all.
9. Add cache status output for disk mode.
10. Add tests for schema mismatch, TTL, root isolation, corrupt entry handling, and manual clear.

### Acceptance criteria

- Disk mode is opt-in.
- Corrupt or old entries are ignored or cleared safely.
- Root isolation is tested.
- Cache clear works.

## Workstream 5: decision record

### Purpose

Leave a durable record of the evaluation whether disk cache ships or remains deferred.

### Target file

- `plans/lsp_phase_16_disk_cache_decision.md` or a section in `architecture/lsp.md`.

### Decision record should include

- benchmark summary,
- privacy/threat-model summary,
- selected option,
- reason for accepting/rejecting disk mode,
- follow-up items if deferred.

### Acceptance criteria

- Future contributors understand why disk cache was or was not added.

## Test matrix

If only evaluating:

```bash
cargo fmt --check
cargo test -p egglsp cache
cargo test --test phase5_context_integration lsp
```

If implementing disk mode:

```bash
cargo fmt --check
cargo test -p egglsp cache
cargo test -p egglsp context_renderer
cargo test --test phase5_context_integration lsp
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

## Final acceptance criteria

Phase 16 is complete when:

- benchmarks answer whether disk cache materially helps,
- privacy/threat model is documented,
- disk cache is either explicitly deferred or implemented as opt-in with root isolation, TTL, caps, schema versioning, and clear commands,
- docs explain the decision,
- memory cache behavior remains unchanged and safe by default.
