# LSP Phase 6-12 Roadmap

Status date: 2026-06-25
Repo: `dbowm91/codegg`
Scope: continued development after the Phase 5 LSP context-packet closeout.

## Current state summary

The LSP integration is now a mature internal subsystem, not a thin experimental tool. The authoritative implementation lives in `crates/egglsp`, while `src/lsp/mod.rs` is a compatibility/re-export shim and `src/tool/lsp.rs` is the model-facing tool boundary. The current surface includes lifecycle management, diagnostics, capability snapshots, bounded read-only semantic operations, preview-only semantic edits, temporary overlays, semantic/security/hunk context packets, preview artifact registration, health snapshots, restart coordination, and model-tier-aware context rendering.

Phase 5 should be treated as architecturally closed. The canonical packet boundary is `egglsp::LspContextPacket`, with provenance, freshness, scoring, budget/truncation metadata, preview IDs, workspace root, generated timestamp, server ID/generation, operational state, and notes. Tool-local DTOs such as `SemanticContextPacket` and `SecurityContextPacket` remain presentation adapters; they must not become competing canonical context models.

The remaining LSP roadmap should focus less on adding raw protocol surface and more on making the existing primitives legible, workflow-oriented, safe to apply, and ergonomic for both human users and model agents.

## Architectural invariants to preserve

The central LSP safety boundary remains unchanged:

```text
read-only semantic operations may execute directly;
mutation-producing operations must remain preview-only until explicitly applied by a higher-level user-approved path.
```

No LSP phase should introduce direct disk mutation through `LspTool`. Rename, formatting, source actions, and code actions must continue to produce preview artifacts only. Applying a preview belongs to a separate mutating path such as `apply_patch`, under Codegg's normal permission and approval model.

`workspace/executeCommand` must remain out of the model-facing LSP path unless a future phase explicitly designs and reviews a separate command execution boundary. Command-only code actions should continue to be rejected rather than silently executed.

All context expansion must stay bounded. New semantic operations should enter through `LspContextPacket` or named bridge functions, with explicit budgets, truncation records, provenance, and freshness metadata. Do not add another parallel packet shape.

Stale evidence must be visible. Diagnostics and context gathered before restarts, base-file changes, workspace-root changes, or server-generation changes should be surfaced as stale/possibly-stale/retained evidence rather than treated as clean current facts.

## Roadmap overview

### Phase 6: polish, docs, status UI, platform support boundary

Goal: make the current LSP system accurately documented and visible in the UI. This phase should not chase new semantic capability. It should reconcile stale documentation, expose health/status details, clarify platform and server-support boundaries, and make preview/freshness/server-state summaries visible to users.

Why this comes first: the repo already has strong internal primitives, but there is visible documentation drift and partial last-mile status plumbing. For example, docs and skills need to agree on server counts and exposed operations; the TUI summary path should stop reporting placeholder zeros for counts that can be derived from packets or registries.

Exit condition: a contributor can read the docs, inspect the TUI/status output, and understand what LSP supports, what is experimental, what server/root is active, whether evidence is stale, and what preview artifacts exist.

Detailed execution plan: `plans/lsp_phase_6_polish_docs_status_plan.md`.

### Phase 7: semantic workflow recipes for repair/review/security/hunks

Goal: turn existing semantic primitives into repeatable agent workflows. This phase should define and implement recipe-level orchestration for repair, review, security review, and hunk-focused navigation.

Why this follows Phase 6: once status and docs are reliable, the next bottleneck is not missing LSP requests; it is knowing which bounded packet to gather for a given workflow and how to render it into model context without excess token burn.

Exit condition: common workflows can ask for named semantic recipes rather than ad-hoc tool calls. Each recipe should define inputs, packet fields, fallback behavior, freshness handling, budget policy, and expected rendering for small/workhorse/frontier models.

Detailed execution plan: `plans/lsp_phase_7_semantic_workflow_recipes_plan.md`.

### Phase 8: preview artifact UX and stale-base lifecycle

Goal: make preview-only semantic edits usable. Preview IDs and registries already exist; this phase should add the user-facing lifecycle around viewing, refreshing, warning, applying, expiring, clearing, and invalidating preview artifacts.

Why this follows recipes: repair/review workflows will increasingly produce rename/format/code-action/source-action previews. Before expanding semantic edits further, the preview lifecycle must be explicit and safe.

Exit condition: users and agents can list previews, inspect affected files and patches, detect stale base files, request recomputation, and hand previews into the mutating apply path without losing provenance.

Detailed execution plan: `plans/lsp_phase_8_preview_artifact_ux_plan.md`.

### Phase 9: lifecycle/workspace/server-health ergonomics — Closed

Status: Closed. All Phase 9 lifecycle commands shipped and documented; preview-apply path is gated by the testable boundary `egglsp::tui_summary::validate_preview_apply` with write-side hardening via `write_preview_apply_plan_atomically_enough()` (per-file SHA-256 recheck before each write, partial failures reported without marking applied).

Completed deliverables:

- `/lsp status` detail view with server/root/generation/state/stderr-tail summary.
- `/lsp restart` and `/lsp stop` commands with root/server scoping.
- Workspace-root explanation and mismatch diagnostics (`/lsp-root`).
- Server capability report display (`/lsp-capabilities`).
- Clear degraded/failing remediation messages.
- `/lsp-preview-apply` applies preview patches to disk with SHA-256 hash revalidation; stale previews are blocked. All gating logic lives in `egglsp::tui_summary::validate_preview_apply` as a testable boundary that returns a typed `PreviewApplyPlan` without mutating disk; the TUI handler performs the actual `std::fs::write` calls and only calls `mark_preview_applied` after every write succeeds. Failed writes leave the preview pending.

**Deferred:** `/lsp-start` and `/lsp-replay-docs` commands deferred to a future phase (no clean scoped API; service auto-starts on demand and handles document replay internally). Per-key server stop uses `shutdown_all` fallback.

### Phase 10: broader semantic operations via bounded packets — Closed

Status: Closed. All five candidate operations are implemented, tested, and packet-first. Each operation lowers into the canonical `LspContextPacket` via named recipe functions in `crates/egglsp/src/workflow_recipes.rs`.

Completed deliverables:

- Impact-analysis packet for symbol rename/refactor review (`execute_impact_analysis`).
- Test-failure repair packet with heuristic symbol extraction (`execute_test_failure_repair`).
- Interface/API boundary packet for trait/interface review (`execute_interface_boundary`).
- Cross-file repair packet with strict file/range/reference caps (`execute_cross_file_repair`).
- Call-neighborhood packet, bounded and non-recursive by default (`execute_call_neighborhood`).

Supporting types: `SymbolTarget` (file + position), `HierarchyDirection` (Incoming/Outgoing/Both).

**Known issue:** `crates/egglsp/src/evidence_collector.rs:1633` emits a
`"references capped"` note for impact analysis when `capped_refs.len() < budget.max_references`
(inverted comparison). Tracked as a follow-up; the underlying
reference-count and budget enforcement are correct.

Remaining: UX polish, renderer section refinements, and expanded recipe composition. See `architecture/lsp.md` Phase 10 section for full details.

### Phase 11: routing/model-tier-aware LSP context policy — Closed

Status: Closed. The repo now has a central `LspContextPolicy` in `crates/egglsp/src/context_policy.rs` that resolves tier, workflow, risk, budget, and stale-evidence decisions.

Completed deliverables:

- Per-workflow default render configs for small/workhorse/frontier tiers (12 recipes × 3 tiers).
- More aggressive compression for small models and routine hunk-local tasks.
- Richer cross-file evidence for frontier review/security tasks.
- Explicit fallback behavior when LSP is unavailable or stale (`StaleEvidencePolicy`, `LspUnavailablePolicy`).
- Metrics or debug logs showing why evidence was included or dropped (`policy_summary`, `TierSource`).
- Per-workflow feature flags: `include_cross_file`, `include_hierarchy`, `include_previews`.

**Known limitation:** `LspContextRenderConfig` does not currently expose
`include_cross_file` / `include_hierarchy` fields, so `to_render_config()`
does not propagate those policy flags. The `RecipeSettings` path
(`to_recipe_settings()`) is unaffected. Consumers needing those flags at
the renderer should use `RecipeSettings` until `LspContextRenderConfig`
is extended.

### Phase 12: optional semantic memory/cache layer — Closed

Status: Closed. Optional bounded in-memory cache for
LSP-derived evidence packets is wired into the production `LspTool` path.
Production cache keys now include request-scoped file hashes via
`collect_cache_file_hashes_for_request()` in `src/tool/lsp.rs`.

Completed deliverables:

- Optional per-workspace semantic cache keyed by root, server ID, server generation, file hash, operation, request fingerprint, capability fingerprint, and budget fingerprint. Production cache keys include request-scoped file hashes via `collect_cache_file_hashes_for_request()`.
- Explicit invalidation on file edit, restart, root change, server change, capability change, and config change (conservative eviction; never silently retained).
- Cache transparency in TUI status (`/lsp-cache-status`) and rendered context notes (`[cache-hit]`).
- Hard caps and TTLs; no unbounded semantic memory growth.

**Production wiring:** `LspTool::lsp_context_for_agent_with_input` routes
through the cache when enabled, via the sync `LspSemanticCache::get` /
`insert` API (rather than `collect_context_cached`) because the cache
guard is `!Send` and cannot cross `.await`. Pattern: lock, lookup, drop
lock, await `collect_context` on miss, lock again, insert. Disk
persistence explicitly deferred.

## Recommended execution order

Proceed in this order:

1. Phase 6: polish/docs/status/platform boundary.
2. Phase 7: semantic workflow recipes.
3. Phase 8: preview artifact UX and stale-base lifecycle.
4. Phase 9: lifecycle/workspace/server-health ergonomics.
5. Phase 10: broader bounded semantic packet operations.
6. Phase 11: deeper model-tier-aware context routing policy.
7. Phase 12: optional semantic memory/cache.

Do not invert Phase 10 and Phase 8. Adding more semantic operations before preview lifecycle hardening will increase the amount of preview-producing behavior without a safe user-facing way to inspect, refresh, or apply it.

Do not start Phase 12 before Phase 9. Semantic memory without trustworthy lifecycle and staleness ergonomics will create subtle stale-evidence failures.

## Near-term risk register

Documentation drift remains a concrete risk. The public architecture doc, skill file, README, and actual server definitions must agree on supported server counts, exposed operations, hidden/internal operations, and current phase status.

Status UI may currently under-report real state. The summary path should avoid placeholder counts when packet or registry data is available, or explicitly label missing fields as unavailable rather than zero.

Tool-local DTOs could drift from the canonical packet. Future contributors should prefer `egglsp::LspContextPacket` and `crates/egglsp/src/bridges.rs` for shared agent/review context.

Preview artifacts could accumulate without clear lifecycle controls. Phase 8 should address expiry, clearing, stale-base recomputation, and apply handoff.

Server-health machinery may remain hard to debug if stderr, generation, root, and restart status are not surfaced in user commands.

## Suggested status labels

Use these labels consistently in docs and plans:

- `Complete`: implemented, tested, and documented.
- `Implemented, UX incomplete`: core primitive exists but user-facing flow is not polished.
- `Planned`: design accepted but not yet implemented.
- `Experimental`: available but not guaranteed beyond pinned versions or specific profiles.
- `Internal only`: crate-level operation exists but is not model-facing.
- `Deferred`: intentionally postponed to a later phase.
