# LSP Diagnostics Freshness and Semantic Context Wiring Plan

## Purpose

The previous LSP pass added the core scaffolding for capability discovery, diagnostic freshness metadata, and a shared semantic context DTO surface. That was the correct foundation, but part of the work is still type-level rather than behavior-level. This pass should wire those types into the runtime path so Codegg can make stronger decisions about whether LSP evidence is fresh, stale, unsupported, or merely unavailable.

The primary outcome should be that diagnostics, semantic context, and security context all carry enough provenance for downstream consumers to avoid treating stale or unsupported LSP evidence as high-confidence data.

## Current State

As of `7eaa413a8ec089ab9a982c88ffc7252a02b8b3e9`:

- `egglsp::capability` contains `LspCapabilitySnapshot`, `LspSemanticOperation`, and `LspUnavailable`.
- `LspTool` exposes a `capabilities` operation that returns a normalized snapshot derived from initialized server capabilities.
- `LspClient.capabilities` is now shared through `Arc<Mutex<Option<ServerCapabilities>>>`.
- `egglsp::diagnostics` defines `LspDiagnosticSnapshot`, `LspDiagnosticFreshness`, and `LspDiagnosticSource`.
- The actual diagnostic cache still stores `HashMap<String, Vec<lsp_types::Diagnostic>>` and does not retain source/freshness/generated-at metadata.
- `DiagnosticsCollector::get_diagnostics_for_file()` still returns `DiagnosticsOutput { diagnostics_may_still_be_warming, diagnostics }`.
- The public `diagnostics` tool output still exposes `diagnostics_may_still_be_warming` but not `freshness`, `source`, or `generated_at_ms`.
- `egglsp::semantic_context` defines shared DTOs, but `src/tool/lsp.rs` still builds local `SemanticContextPacket` and `SecurityContextPacket` structures directly.

## Non-Goals

Do not make LSP mandatory.

Do not require live language servers in unit tests.

Do not add mutating edit/apply behavior.

Do not expand security review into offensive automation.

Do not rewrite the entire LSP tool in one pass.

Do not remove the existing public `semanticContext` or `securityContext` response shape unless all consumers are updated in the same change.

## Phase 1 — Wire Diagnostic Metadata Into the Cache

Replace the bare diagnostic cache with a metadata-carrying cache entry.

Suggested internal type:

```rust
#[derive(Debug, Clone)]
pub struct DiagnosticCacheEntry {
    pub diagnostics: Vec<lsp_types::Diagnostic>,
    pub received_at: std::time::Instant,
    pub source: LspDiagnosticSource,
    pub content_version: Option<i32>,
}
```

Implementation notes:

- Update `LspClient.diagnostics` from `Arc<Mutex<HashMap<String, Vec<Diagnostic>>>>` to `Arc<Mutex<HashMap<String, DiagnosticCacheEntry>>>`.
- Update `dispatch_notification()` to store `DiagnosticCacheEntry { source: Pushed, received_at: Instant::now(), ... }`.
- If `publishDiagnostics` includes a version, preserve it as `content_version` when available.
- Keep conversion helpers so older callers can still retrieve `Vec<Diagnostic>` until all surfaces are migrated.
- Avoid exposing `Instant` directly in serialized output. Convert to a stable age or wall-clock millisecond field at snapshot construction time.

Acceptance criteria:

- Diagnostics notification ingestion records source and receipt time.
- Existing LSP operations still compile and retain old behavior where needed.
- Unit tests cover `dispatch_notification()` storing metadata rather than a raw vector.

## Phase 2 — Define Freshness Classification Rules in Code

Move freshness semantics from documentation into deterministic code.

Suggested method:

```rust
impl LspClient {
    pub async fn diagnostic_snapshot(&self, uri: &str) -> LspDiagnosticSnapshot;
}
```

Classification rules:

- `Unavailable`: no cache entry exists for the URI.
- `PossiblyStale`: the file has been opened or changed more recently than the last diagnostics entry, but the server may still be processing.
- `Fresh`: diagnostics entry is present and no newer open/change/save timestamp exists for that URI.
- `Stale`: server lifecycle invalidated the cache, the workspace root changed, or an explicit invalidation marker exists.

Implementation notes:

- The existing `last_opened_at` map can become `last_synced_at` or `last_content_change_at`; the name should reflect `didOpen` and `didChange`, not just open events.
- Consider tracking a per-client `diagnostics_invalidated_at: Option<Instant>` set on restart/shutdown/error paths.
- `save_file()` should also update the content-change timestamp unless there is a separate save timestamp with clear semantics.
- Keep `diagnostics_may_still_be_warming()` as a compatibility helper, but implement it from the new freshness state if possible.

Acceptance criteria:

- Tests cover `Unavailable -> PossiblyStale -> Fresh -> PossiblyStale` transitions without a live LSP server.
- Tests cover server invalidation producing `Stale` for previously available diagnostics.
- The freshness classification is documented in `architecture/lsp.md` and `.opencode/skills/lsp/SKILL.md`.

## Phase 3 — Expose Diagnostic Snapshots Through Service and Collector

Make `LspDiagnosticSnapshot` the main diagnostic read model.

Implementation steps:

- Add `LspService::get_diagnostic_snapshot_for_key(key, uri)`.
- Change `DiagnosticsCollector` to expose a new method:

```rust
pub async fn get_diagnostic_snapshot_for_file(
    &self,
    file_path: &Path,
) -> Result<LspDiagnosticSnapshot, LspError>;
```

- Keep `get_diagnostics_for_file()` temporarily as a compatibility wrapper that derives `DiagnosticsOutput` from the snapshot.
- Populate `generated_at_ms`, `source`, and `freshness` in the snapshot.
- Ensure file paths in snapshots are human-readable paths, not only `file://` URIs.

Acceptance criteria:

- Existing diagnostics consumers can remain on the wrapper temporarily.
- New tests assert snapshot metadata and compatibility wrapper behavior.
- No live LSP server is required for snapshot tests; use fake cache entries or unit-level helpers.

## Phase 4 — Update Public `diagnostics` Tool Output

Expose freshness metadata to tool callers.

Suggested output shape under `results`:

```json
{
  "diagnostics_may_still_be_warming": false,
  "freshness": "Fresh",
  "source": "Pushed",
  "generated_at_ms": 123456789,
  "usable_evidence": true,
  "diagnostics": []
}
```

Rules:

- Preserve `diagnostics_may_still_be_warming` for compatibility.
- Add `freshness`, `source`, `generated_at_ms`, and `usable_evidence`.
- `usable_evidence` should call `LspDiagnosticSnapshot::is_usable_evidence()`.
- If freshness is `Stale` or `Unavailable`, do not silently present an empty diagnostics list as proof that the file is clean.

Acceptance criteria:

- Public JSON includes freshness metadata.
- Snapshot tests verify stale/unavailable diagnostics are distinguishable from clean fresh diagnostics.
- Tool schema or description mentions freshness metadata.

## Phase 5 — Thread Freshness Into `semanticContext` and `securityContext`

Semantic consumers should know whether diagnostic evidence is fresh enough to trust.

Implementation notes:

- Add diagnostic metadata to `SemanticContextPacket` without breaking existing fields:

```rust
struct DiagnosticEvidenceMeta {
    freshness: LspDiagnosticFreshness,
    source: LspDiagnosticSource,
    generated_at_ms: i64,
    usable_evidence: bool,
}
```

- Add an optional field such as `diagnostic_evidence` or `diagnostics_meta` to `SemanticContextPacket` and `SecurityContextPacket`.
- For `securityContext`, add a note when diagnostics are stale or unavailable:
  - `diagnostics stale: treating diagnostics as low-confidence evidence`
  - `diagnostics unavailable: no LSP diagnostic evidence available`
- Avoid filtering diagnostics solely because they are stale. Preserve them with metadata so the reviewer can decide how to use them.

Acceptance criteria:

- `semanticContext` includes diagnostic freshness metadata.
- `securityContext` includes diagnostic freshness metadata and adds notes for stale/unavailable diagnostics.
- Security review does not treat stale diagnostics as clean/negative evidence.

## Phase 6 — Start Migrating Toward Shared Semantic Context DTOs

Do not attempt a full rewrite. Establish adapters first.

Implementation steps:

- Add conversion helpers between existing local tool summaries and `egglsp::semantic_context` DTOs:
  - `DiagnosticSummary -> FileDiagnostic` or the reverse, depending on chosen boundary.
  - `SymbolSummary -> SemanticSymbolSummary`.
  - `LocationSummary -> SemanticLocation`.
  - local call/type hierarchy summaries into compact shared summaries where possible.
- Add a builder function that constructs `egglsp::SemanticContextResponse` from the existing `semanticContext` collection path.
- Keep the current public `SemanticContextPacket` response for compatibility in this pass, but include the shared response internally or behind an optional field only if the shape is stable.
- Mark local packet structures as tool presentation DTOs, not the domain API.

Acceptance criteria:

- `egglsp::SemanticContextResponse` is produced by at least one runtime path or adapter test, not only unit-constructed in DTO tests.
- Existing public `semanticContext` output remains backward compatible.
- Documentation distinguishes domain DTOs from tool presentation DTOs.

## Phase 7 — Capability-Gated Fail-Soft Operation Calls

Use `LspCapabilitySnapshot` before optional expensive operations where it is safe to do so.

Targets:

- `semanticContext` optional definitions/references/hierarchy sections.
- `securityContext` definitions/references/call hierarchy enrichment.
- Direct `callHierarchy` and `typeHierarchy` calls, where appropriate.

Rules:

- Unsupported optional enrichment should append `LspUnavailable` metadata or notes, not fail the whole request.
- Direct operation calls may still return a clear tool error if the operation was explicitly requested and unavailable, but structured unavailable output is preferable where compatible.
- Be cautious with heuristics: type hierarchy currently has a weak capability signal. Do not over-trust the call-hierarchy heuristic as a hard guarantee.

Acceptance criteria:

- Optional sections fail soft on known unsupported capabilities.
- Direct `capabilities` output remains stable.
- Tests cover unsupported capability fallback without live LSP.

## Phase 8 — Documentation and Test Commands

Update documentation:

- `architecture/lsp.md`
- `.opencode/skills/lsp/SKILL.md`
- `AGENTS.md` verified facts only if new facts are actually true after implementation

Suggested checks:

```bash
cargo fmt --all
cargo test -p egglsp
cargo test --lib lsp
cargo test --all --workspace
```

If the full workspace test set is too expensive or currently flaky, record the exact narrower command set that passed and the reason the full command was skipped.

## Review Checklist

Before considering this pass complete, verify:

- A clean fresh diagnostics vector is distinguishable from diagnostics unavailable.
- A stale non-empty diagnostics vector is distinguishable from fresh diagnostics.
- `diagnostics_may_still_be_warming` is derived from freshness or remains semantically consistent with it.
- `semanticContext` and `securityContext` preserve diagnostic metadata.
- Security review does not use stale/unavailable diagnostics as negative evidence.
- No live LSP server is required for the new unit tests.
- Existing public response shapes remain backward compatible unless all call sites are updated.

## Expected Follow-Up

After this pass, the next likely LSP target is broader semantic-context consolidation: move presentation-specific packet assembly out of `src/tool/lsp.rs`, make the shared `egglsp::SemanticContextResponse` the internal source of truth, then let individual tool operations adapt it into user-facing JSON shapes. That should be a separate refactor once diagnostic freshness is behaviorally reliable.
