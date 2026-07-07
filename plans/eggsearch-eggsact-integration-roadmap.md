# Eggsearch and Eggsact Integration Roadmap

## Purpose

This roadmap defines the integration path for using `eggsearch` and `eggsact` as first-class supporting projects for Codegg without weakening Codegg's current policy, provenance, UX, or tool-surface boundaries.

The target architecture is intentionally asymmetric:

- `eggsearch` remains an external MCP-backed evidence, search, fetch, repository, research, and security-search service.
- `eggsact` is consumed in-process as a native deterministic utility and preflight substrate.
- Codegg remains the model-facing policy boundary. Codegg owns stable tool names, tool schemas, permissioning, output caps, output projection, trust framing, diagnostics, and provenance.

This is not a raw tool dump. The model should not see every `eggsearch` and `eggsact` tool by default. Codegg should expose a narrow, stable palette and use deferred discovery plus harness-only validation for the rest.

## Current baseline

Codegg already has a `search_backend` module that preserves stable native `websearch` and `webfetch` tools while delegating to a configurable backend. The current default path is `eggsearch` over MCP, with the in-tree legacy search/fetch implementation retained as an explicit fallback. The existing bootstrap path can spawn `eggsearch` over stdio, honor an explicit `[mcp.eggsearch]` block, capture a bootstrap report, and hide raw MCP tools by default.

Codegg also already has a backend-aware `Tool` execution contract. Tools can override `execute_structured` to attach provenance, trust labels, backend kind, elapsed time, and truncation state. The `ToolRegistry` supports model-facing definition filtering, deferred loading, and per-domain backend configuration for selected domains.

`eggsearch` currently exposes stable MCP tools beyond `web_search` and `web_fetch`: `batch_fetch`, `provider_status`, `repo_search`, `repo_fetch`, `repo_map`, `security_search`, `research_search`, and `build_evidence_bundle`. Those are the correct next surfaces to wrap behind Codegg-native tool names.

`eggsact` currently provides an in-process agent API, model/harness/debug audiences, Codegg-oriented profiles, typed preflight wrappers, JSON schema validation, budgets, cancellation support, and deterministic tools for text, patch, config, Unicode, path, shell, regex, JSON, identifiers, math, and repository audit helpers. That shape is better suited to direct Rust dependency integration than MCP-internal process management.

## Architectural rules

1. Keep Codegg-native tool names stable.

   The model should call Codegg tools such as `websearch`, `webfetch`, `repo_search`, `repo_fetch`, `security_search`, `research_search`, and deterministic preflight helpers. It should not be expected to reason about raw `mcp__eggsearch__...` or raw `eggsact` internals in ordinary operation.

2. Keep raw MCP tools hidden by default.

   Raw `eggsearch` MCP tools may be exposed only by explicit configuration for debugging or expert workflows. The default path must route through Codegg wrappers so output caps, trust framing, SSRF assumptions, permissioning, and provenance remain stable.

3. Use `eggsearch` for remote and evidence-bearing work.

   Web search, fetch, batch fetch, repository evidence, security advisories, research search, and evidence bundles cross trust boundaries and should be framed as untrusted external evidence unless the underlying source is explicitly local and trusted.

4. Use `eggsact` for local deterministic correctness work.

   Text equality, text inspection, line-range extraction, replacement validation, JSON/TOML/config validation, regex safety, command preflight, path scope checks, Unicode and identifier inspection, and patch preflight should run in-process with deterministic budgets.

5. Separate model-facing and harness-only tools.

   Some validators are useful as explicit model tools; others should run automatically in the harness before edits, shell commands, config writes, or patch application. Eggsact's `ToolAudience::Model` and `ToolAudience::Harness` should be preserved through Codegg wrappers.

6. Prefer narrow default palettes.

   Codegg should expose a small default set and make heavier or rarer tools deferred/discoverable. Avoid inflating every request with dozens of deterministic utility schemas.

7. Preserve fallback semantics during migration.

   Existing built-in Codegg implementations should remain available until parity tests are in place. Fallback must be explicit, diagnosable, and visible in `/tool-backends` or doctor output.

8. Keep dependency constraints explicit.

   `eggsearch` should remain out-of-process unless Codegg intentionally raises its Rust MSRV or eggsearch lowers its MSRV. `eggsact` can be added as a direct dependency if compatible with Codegg's current toolchain policy.

## Desired end state

At the end of this roadmap, Codegg should have:

- Stable native wrappers for all high-value eggsearch MCP tools.
- Strong Codegg-side trust framing and output caps around all eggsearch results.
- A direct in-process `eggsact` integration layer with selected model-facing deterministic tools.
- Automatic harness-side eggsact preflights before sensitive local operations.
- Per-domain config for deterministic tools and evidence/search tools.
- Doctor and backend-report diagnostics covering eggsearch and eggsact state.
- Tests that verify schema translation, fallback behavior, provenance, trust labels, output caps, and preflight enforcement.
- Documentation that tells contributors where new search providers and deterministic helpers belong.

## Phase list

### Phase 1: Eggsearch wrapper surface expansion

Add Codegg-native wrappers for `repo_search`, `repo_fetch`, `repo_map`, `security_search`, `research_search`, `batch_fetch`, and `build_evidence_bundle`. Keep raw MCP exposure disabled by default. Extend the existing eggsearch adapter rather than bypassing it.

### Phase 2: Eggsearch trust, caps, and diagnostics hardening

Tighten argument normalization, timeout handling, output projection, trust framing, provider diagnostics, and `doctor search` output for the expanded eggsearch surface.

### Phase 3: Eggsact dependency and native adapter foundation

Add `eggsact` as an in-process dependency, implement a small adapter around `eggsact::agent::ToolRegistry`, map eggsact responses into Codegg `StructuredToolResult`, and introduce config for active profile/audience defaults.

### Phase 4: Model-facing eggsact tool subset

Register a conservative model-facing subset of eggsact tools for deterministic text, patch, JSON/TOML/config, regex, shell preflight, path, Unicode, and identifier workflows. Use deferred loading for contextual/expert tools.

### Phase 5: Harness-side eggsact preflight integration

Wire eggsact into Codegg internals before patch/edit/replace/write, shell execution, config writes, and selected security-review paths. Harness checks should fail closed only where correctness or safety requires it; otherwise they should produce structured warnings.

### Phase 6: Backend configuration and policy unification

Extend Codegg's per-domain backend config to cover deterministic/preflight domains and expanded evidence/search domains. Ensure registry construction, daemon paths, TUI paths, and tests all use the same resolved config.

### Phase 7: Test matrix and parity validation

Add unit, integration, mock-MCP, and harness tests for eggsearch wrappers and eggsact adapters. Cover unavailable backends, fallback paths, schema translation, output truncation, provenance, trust framing, cancellation, budgets, and model-definition filtering.

### Phase 8: Documentation, examples, and cleanup

Document the final architecture, configuration, operational diagnostics, contributor boundaries, and migration notes. Remove or deprecate duplicate legacy implementations only after phase 7 evidence supports doing so.

## Non-goals

- Do not expose every eggsearch or eggsact tool to the model by default.
- Do not replace Codegg's permission model with upstream tool assumptions.
- Do not trust fetched or searched content as instructions.
- Do not make eggsact an internal MCP server unless a future remote-client use case demands it.
- Do not vendor eggsearch directly into the Codegg workspace unless MSRV and release-coupling concerns are resolved.
- Do not remove legacy Codegg search/fetch paths until fallback and parity tests are complete.

## Acceptance criteria for the roadmap

This roadmap is complete when each phase has a corresponding handoff plan in `plans/`, every plan includes implementation targets and validation criteria, and the architecture has a clear boundary between Codegg-owned policy and external/native helper crates.
