# LSP Phase 12 Plan: Optional Semantic Memory and Cache Layer

Status date: 2026-06-26
Phase type: optional caching / semantic memory / performance
Prerequisites: Phase 9 lifecycle/freshness ergonomics and Phase 11 context policy complete enough to avoid stale-evidence ambiguity.

## Purpose

Phase 12 should add an optional semantic cache/memory layer for LSP-derived evidence only after lifecycle, freshness, root, and model-tier policy are trustworthy. The cache should improve latency and reduce repeated semantic queries without making stale evidence look fresh.

This phase is explicitly optional. Codegg should remain correct and useful without semantic caching. The cache must never become the authoritative source of truth when the server generation, workspace root, file content, capability snapshot, or request shape has changed.

## Current baseline

The repo already has:

- `LspContextPacket` with provenance, server ID, server generation, request, budget, truncation, and notes.
- Evidence freshness variants such as fresh, possibly stale, stale, retained after restart, stale after edit, server generation mismatch, and unknown.
- Preview artifact hashes and stale-base refresh.
- Lifecycle health states and restart generation semantics.
- Model-tier rendering and planned policy layer.

Phase 12 should build on those semantics, not bypass them.

## Non-goals

Do not add cache before lifecycle/generation/root behavior is clear.

Do not cache raw unbounded LSP JSON.

Do not cache mutation-producing operations as applyable edits without hash validation.

Do not persist sensitive content by default without explicit user config.

Do not make cached evidence indistinguishable from fresh evidence.

Do not allow unbounded growth.

Do not cache across workspaces unless roots and repository identities are explicit and verified.

## Core design principles

1. Correctness over speed.
2. Cached evidence is always provenance-carrying.
3. Cache hits must be labeled as cache hits in debug/status or packet notes.
4. Cache validity must depend on file content/version, server identity/generation, operation, request inputs, and capability snapshot.
5. Cache entries must have TTL and size caps.
6. Cache must be optional and easy to disable.
7. Cache invalidation must be conservative.

## Workstream 1: decide cache scope and storage mode

### Problem

Semantic data ranges from cheap diagnostics to expensive cross-file impact packets. Not all data should be cached, and persistent caching may create privacy/staleness concerns.

### Target files

- new module candidate: `crates/egglsp/src/cache.rs`
- `crates/egglsp/src/context.rs`
- `crates/egglsp/src/evidence_collector.rs`
- config schema
- docs

### Storage modes

Support at least one mode first:

- `disabled`: default if conservative.
- `memory`: process-local bounded cache, safe default for experimentation.
- `disk`: optional later, requires explicit config and stronger invalidation.

Recommended initial implementation: memory-only cache with explicit config to enable. Disk persistence can be a later Phase 12 substep after memory cache semantics are proven.

### Cacheable candidates

Good initial candidates:

- rendered `LspContextPacket` for a specific request with exact file hashes,
- document symbols for a file version,
- diagnostics snapshots by file/server generation,
- bounded impact-analysis packets from Phase 10,
- hunk packets keyed by file hash plus hunk coordinates.

Avoid initially:

- completion items,
- signature help tied to rapidly changing cursor state,
- preview artifacts intended for apply handoff,
- command/code-action metadata that may expire server-side,
- anything lacking file hash/version provenance.

### Acceptance criteria

- Cache scope is explicitly documented.
- Memory cache exists or disk cache is deliberately deferred.
- Default behavior is safe and disabled or conservative.

## Workstream 2: cache key design

### Problem

A semantic cache is only safe if cache keys encode every input that can change the answer.

### Target files

- `crates/egglsp/src/cache.rs`
- `crates/egglsp/src/context.rs`
- tests

### Required key fields

At minimum:

- workspace root canonical path or stable root ID,
- server ID,
- server generation or generation policy,
- server/profile version if available,
- operation/request kind,
- request-specific fields: file, line, column, symbol, hunk ranges, changed files, risk mode,
- file content hashes for every input file,
- config/profile hash if capability or root behavior can alter results,
- budget shape if truncation affects result,
- model-tier/render policy if rendered text is cached instead of packet.

### Recommended approach

Cache canonical packets, not rendered strings, unless rendering cost becomes a problem. Packet caching lets Phase 11 policy render differently by model/tier without invalidating semantic evidence unnecessarily.

Suggested types:

```rust
pub struct LspCacheKey {
    pub workspace_root: PathBuf,
    pub server_id: String,
    pub operation: String,
    pub request_fingerprint: String,
    pub input_hashes: BTreeMap<PathBuf, String>,
    pub capability_fingerprint: Option<String>,
    pub budget_fingerprint: String,
}
```

Do not include server generation directly if the entry is intentionally reusable across restarts and file hashes/capabilities match. If generation differs, mark freshness as `RetainedAfterRestart` or `ServerGenerationMismatch`, not `Fresh`.

### Acceptance criteria

- Cache keys include file hashes and request fingerprints.
- Cache entries cannot cross workspace roots accidentally.
- Tests prove different file content, root, server ID, request kind, and budget produce different keys.

## Workstream 3: cache entry metadata and freshness transitions

### Problem

Cache hits must be converted into correct freshness labels. A cache hit after restart is not the same as fresh live LSP evidence.

### Target files

- `crates/egglsp/src/cache.rs`
- `crates/egglsp/src/context.rs`
- `crates/egglsp/src/evidence_collector.rs`
- `crates/egglsp/src/context_renderer.rs`

### Required metadata

Each cache entry should store:

- cached packet or items,
- created timestamp,
- last used timestamp,
- workspace root,
- server ID,
- server generation at collection time,
- request fingerprint,
- input hashes,
- capability fingerprint,
- budget fingerprint,
- TTL,
- hit count,
- original freshness summary.

### Freshness rules

- Same file hashes + same server generation: may remain `Fresh` if original evidence was fresh and TTL valid.
- Same file hashes + different server generation: downgrade to `RetainedAfterRestart` or `ServerGenerationMismatch`.
- Changed file hash: invalidate; do not return entry.
- Missing file: invalidate or return stale operational note only; prefer invalidate.
- Capability fingerprint changed: invalidate or downgrade with explicit note; prefer invalidate.
- TTL expired: invalidate.
- Budget changed narrower: either re-enforce budget on cached packet or treat as miss.
- Budget changed wider: treat as miss unless original packet retained enough dropped data, which it likely does not.

### Acceptance criteria

- Cache hits cannot be mislabeled fresh after restart/generation mismatch.
- File changes invalidate entries.
- Expired entries are not returned.
- Tests cover same-generation hit, restart downgrade, changed-file miss, TTL expiry.

## Workstream 4: memory cache implementation

### Target files

- `crates/egglsp/src/cache.rs`
- `crates/egglsp/src/lib.rs`
- tests

### Proposed implementation

Add a small bounded LRU-ish memory cache:

```rust
pub struct LspSemanticCache {
    max_entries: usize,
    max_bytes: usize,
    ttl: Duration,
    entries: HashMap<LspCacheKey, LspCacheEntry>,
    order: VecDeque<LspCacheKey>,
}
```

Keep it simple. Exact LRU is not required if deterministic cap eviction is tested.

### Implementation steps

1. Add cache module behind always-available code, but disabled unless used by service/collector.
2. Implement insert/get/remove/clear/stats.
3. Track approximate entry size via serialized packet length or rough byte counts.
4. Enforce max entries and max bytes.
5. Add stats:
   - entries,
   - bytes,
   - hits,
   - misses,
   - stale misses,
   - evictions.
6. Add unit tests for cap eviction, TTL, hit/miss, clear, stats.

### Acceptance criteria

- Memory cache is bounded.
- Tests prove eviction and TTL behavior.
- Cache can be disabled or unused by default.

## Workstream 5: integration with evidence collection

### Problem

The cache should integrate at a safe layer. It should not be mixed into every low-level LSP operation before semantics are proven.

### Target files

- `crates/egglsp/src/evidence_collector.rs`
- `crates/egglsp/src/workflow_recipes.rs`
- `src/tool/lsp.rs`
- config wiring

### Recommended initial integration

Wrap high-level packet collection:

```text
LspContextRequest + budget + root/server/hash metadata -> cache lookup -> collect_context on miss -> cache insert if eligible
```

Do not cache every raw provider method initially.

### Implementation steps

1. Add `collect_context_cached()` alongside `collect_context()` rather than modifying all callers immediately.
2. Require caller to provide cache, root, and file hash provider.
3. On cache hit, adjust freshness/notes according to rules.
4. On miss, call existing collector and insert if eligible.
5. Integrate with selected Phase 10 high-cost operations first.
6. Keep default production path on uncached collector until tests are strong, then enable behind config.

### Acceptance criteria

- Cached collection is opt-in.
- Existing uncached behavior remains unchanged.
- Cache hit/miss notes are visible in packet notes or debug logs.
- Tests compare cached and uncached output for stable inputs.

## Workstream 6: invalidation hooks

### Problem

Cache correctness depends on invalidation when workspace, files, capabilities, or lifecycle changes.

### Target files

- `crates/egglsp/src/cache.rs`
- `crates/egglsp/src/document_sync.rs`
- `crates/egglsp/src/service.rs`
- `crates/egglsp/src/restart.rs`
- config/root detection code

### Invalidation events

- file content changed,
- document opened/changed/saved/closed if versioning is available,
- server restart/generation changed,
- server ID/profile changed,
- workspace root changed,
- capability snapshot changed,
- LSP config changed,
- cache TTL expired,
- manual clear.

### Implementation steps

1. Add manual `clear_cache()` and `clear_cache_for_root()`.
2. Add targeted invalidation by file path/hash if practical.
3. On restart, do not necessarily clear all entries; either downgrade on read or clear by policy. Prefer conservative clear first if downgrading is not fully tested.
4. Add `/lsp-cache-clear` only if user-facing cache exists.
5. Add tests for file/root/server invalidation.

### Acceptance criteria

- Cache never survives unsafe changes as fresh evidence.
- Manual clear exists if cache is user-visible.
- Invalidation behavior is documented.

## Workstream 7: disk cache evaluation and optional implementation

### Problem

Disk cache can improve startup and repeated sessions but creates privacy, stale, and schema-migration risks.

### Recommendation

Defer disk cache unless memory cache is stable and there is clear performance need.

If implemented, require explicit config:

```toml
[lsp.semantic_cache]
mode = "disk"
max_entries = 1000
max_mb = 64
ttl_seconds = 86400
```

### Disk cache requirements

- schema version,
- root-scoped path under Codegg cache dir,
- no secrets in keys/logs,
- clear command,
- TTL and max size enforcement,
- migration/drop-on-version-mismatch behavior,
- opt-in docs warning that source-derived semantic evidence may be stored.

### Acceptance criteria if implemented

- Disk cache is opt-in.
- Schema version mismatch clears or ignores old entries.
- User can clear disk cache.
- Docs disclose source-derived content storage.

## Workstream 8: TUI/status/debug surface

### Target files

- `src/tui/command.rs`
- `src/tui/app/mod.rs`
- `src/tool/lsp.rs`
- `crates/egglsp/src/cache.rs`

### Proposed commands

- `/lsp-cache-status`
- `/lsp-cache-clear [--root <path>|--all]`
- `/lsp-cache-policy`

If cache remains internal/experimental, expose debug logs instead of commands.

### Status should show

- mode: disabled/memory/disk,
- entries,
- approximate bytes,
- hits/misses,
- evictions,
- stale invalidations,
- TTL,
- root scope.

### Acceptance criteria

- Users can tell whether cache is enabled.
- Users can clear cache.
- Debugging cache behavior does not require reading internals.

## Workstream 9: privacy and security review

### Problem

Semantic cache can store source-derived evidence. This has privacy and security implications.

### Requirements

- Disabled or memory-only by default unless explicitly configured.
- Disk cache opt-in only.
- No cache across unrelated workspace roots.
- No raw secrets intentionally stored beyond what source excerpts already contain in packets.
- Clear command.
- Docs warning for disk mode.
- Security review tests/checklist for stale and cross-root leakage.

### Acceptance criteria

- Cache cannot leak evidence across roots in tests.
- Disk mode, if added, requires explicit config.
- Docs explain what is stored.

## Workstream 10: documentation

Update:

- `architecture/lsp.md`
- `.opencode/skills/lsp/SKILL.md`
- config docs if config added
- `CHANGELOG.md`

Document:

- cache mode defaults,
- cache key design,
- freshness transitions,
- invalidation events,
- status/clear commands,
- privacy considerations,
- known limitations.

## Test matrix

Required focused tests:

```bash
cargo fmt --check
cargo test -p egglsp cache
cargo test -p egglsp evidence_collector
cargo test -p egglsp context_renderer
```

Recommended broader checks:

```bash
cargo test -p egglsp
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

If disk cache is implemented, add tests for schema version mismatch, root isolation, TTL, and manual clear.

## Final acceptance criteria

Phase 12 is complete when:

- semantic cache is optional and safe by default,
- cache keys include root, operation/request fingerprint, file hashes, capability/budget identity, and server identity as needed,
- cache hits preserve or downgrade freshness correctly,
- file/root/server/config changes invalidate or downgrade entries conservatively,
- memory cache is bounded and tested,
- disk cache is either explicitly deferred or opt-in with privacy docs,
- users can inspect and clear cache if it is enabled,
- cached evidence never appears as fresh when it should be stale/retained/unknown.
