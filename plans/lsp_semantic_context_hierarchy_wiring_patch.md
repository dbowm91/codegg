# LSP Semantic Context Hierarchy Wiring Patch Plan

## Purpose

The semantic-context cleanup pass at `ac4449e38cf6bd3fb43218dc526044ad6f8f11df` materially improved the LSP semantic read model, but two request-wiring gaps remain before hunk/source navigation should start.

This is a narrow patch plan. It should not expand scope into hunk/source navigation or another broad DTO redesign.

## Current State

The codebase now has:

- explicit `SemanticContextRequest::include_call_hierarchy` and `include_type_hierarchy` flags;
- default hierarchy flags set to `false`;
- typed `SemanticDiagnosticEvidence` using `LspDiagnosticFreshness` and `LspDiagnosticSource`;
- shared hierarchy DTOs that can represent detailed call/type hierarchy data;
- `SemanticContextCollector` gating hierarchy collection on request flags;
- `SemanticContextPacket::from_semantic_response()` adapting shared hierarchy DTOs into the existing public packet shapes;
- improved truncation propagation via packet limits and section truncations;
- `securityContext` consuming shared semantic evidence for diagnostics/symbols/definitions/references.

Remaining issues:

1. `semanticContext` reads `include_call_hierarchy` and `include_type_hierarchy`, but does not appear to pass those flags into `SemanticContextRequest` before `collector.collect(request)`.
2. `securityContext` uses the shared semantic response, but does not appear to set `request.include_call_hierarchy = settings.include_call_hierarchy && has_position`, so shared call hierarchy may never populate.
3. The old reverse adapter `build_semantic_context_response(packet)` remains as dead code even though the architecture now flows response -> packet.
4. Regression tests should prove hierarchy emission still works through the public tool-facing paths.

## Non-Goals

Do not implement hunk/source navigation in this pass.

Do not change public JSON shapes unless tests reveal an unavoidable bug.

Do not move overlay ownership again.

Do not force source-action ownership into the collector.

Do not alter call expansion behavior beyond preserving existing security-context semantics.

Do not require live language servers in new tests.

## Phase 1 — Wire Hierarchy Flags Into `semanticContext` Request

Problem:

The `semanticContext` handler validates hierarchy flags and later strips hierarchy from the packet when flags are false, but the collector only builds hierarchy when request flags are true. If the request flags are never set, `include_call_hierarchy=true` and `include_type_hierarchy=true` can silently return no hierarchy.

Implementation:

In the `semanticContext` branch of `src/tool/lsp.rs`, after computing:

```rust
let include_call_hierarchy = parsed.include_call_hierarchy.unwrap_or(false);
let include_type_hierarchy = parsed.include_type_hierarchy.unwrap_or(false);
```

set those values on the request before `collector.collect(request)`:

```rust
request.include_call_hierarchy = include_call_hierarchy;
request.include_type_hierarchy = include_type_hierarchy;
```

or use the existing builders:

```rust
request = request
    .with_call_hierarchy(include_call_hierarchy)
    .with_type_hierarchy(include_type_hierarchy);
```

Keep the existing validation that hierarchy requires both line and column.

Acceptance criteria:

- `semanticContext` with `include_call_hierarchy=false` does not request call hierarchy from the collector.
- `semanticContext` with `include_call_hierarchy=true` and a valid position passes that request to the collector.
- `semanticContext` with `include_type_hierarchy=true` and a valid position passes that request to the collector.
- Existing public output shape remains unchanged.

## Phase 2 — Wire Security Call-Hierarchy Flag Into Shared Request

Problem:

`securityContext` now consumes a shared semantic response, but call hierarchy is not requested from the collector even when `settings.include_call_hierarchy` is true. The later code uses `shared_call_hierarchy`, so the shared response must be populated.

Implementation:

In the `securityContext` branch, after creating the `SemanticContextRequest`, set:

```rust
request.include_call_hierarchy = settings.include_call_hierarchy && has_position;
request.include_type_hierarchy = false;
```

Type hierarchy is not currently part of security context, so keep it off unless a future security preset explicitly needs it.

Keep security-specific call expansion handler-local. Call expansion is broader than the shared compact call hierarchy and can remain as-is.

Acceptance criteria:

- `securityContext` with a position and default/preset call hierarchy enabled requests shared call hierarchy from the collector.
- `securityContext` with no position does not request call hierarchy and preserves the existing validation behavior.
- `securityContext` call expansion remains handler-local and unchanged.
- Public `securityContext` JSON shape remains unchanged.

## Phase 3 — Remove or Quarantine Dead Reverse Adapter

Problem:

`build_semantic_context_response(packet)` still exists as dead code. The architecture is now `SemanticContextResponse -> SemanticContextPacket`, not `SemanticContextPacket -> SemanticContextResponse`.

Implementation options:

Preferred:

- Remove `build_semantic_context_response(packet)` entirely.
- Remove any associated `From<&...>` implementations that are now only used by that function.

Fallback:

- If removing it causes test churn, keep it but move it behind `#[cfg(test)]` and rename it to make the transitional/test-only role explicit.

Acceptance criteria:

- No dead reverse adapter remains in production code.
- No unused conversion impls remain solely to support the dead adapter.
- `cargo clippy` does not require `#[allow(dead_code)]` for this path.

## Phase 4 — Regression Tests

Add narrowly scoped tests that do not require live LSP servers.

Suggested tests:

### Request construction / adapter tests

If the request-building logic is extractable, add tests for:

- `semantic_context_request_sets_call_hierarchy_flag`
- `semantic_context_request_sets_type_hierarchy_flag`
- `security_context_request_sets_call_hierarchy_when_enabled`
- `security_context_request_does_not_set_call_hierarchy_without_position`

If request building remains inline in `execute`, use adapter-level tests that construct `SemanticContextResponse` with populated hierarchy and verify packet output.

### Packet adapter tests

- `semantic_packet_adapts_shared_call_hierarchy`
- `semantic_packet_adapts_shared_type_hierarchy`
- `security_context_uses_shared_call_hierarchy_when_present`

### Negative tests

- `semantic_context_hierarchy_flags_false_omits_hierarchy`
- `semantic_context_hierarchy_requested_without_position_errors`

Implementation guidance:

- Prefer fake/static `SemanticContextResponse` fixtures.
- Avoid starting `rust-analyzer` or any live LSP server.
- If testing the full `execute` path is hard without a server, isolate request construction into a helper function and test that helper.

Acceptance criteria:

- Tests would fail if hierarchy flags were not propagated into `SemanticContextRequest`.
- Tests would fail if shared hierarchy stopped adapting into public packets.
- No live language server is required.

## Phase 5 — Documentation Touch-Up

Only update docs if needed after the patch.

Files:

- `architecture/lsp.md`
- `.opencode/skills/lsp/SKILL.md`
- `AGENTS.md`, only if a verified fact changed

Docs should remain modest:

- `SemanticContextCollector` owns shared semantic evidence.
- `semanticContext` requests hierarchy only when include flags are true.
- `securityContext` reuses shared call hierarchy when enabled, but call expansion remains security-specific.
- Overlay and preview-rich source actions remain handler-local unless later changed.

Acceptance criteria:

- Docs do not overclaim full overlay/source-action consolidation.
- Docs accurately describe hierarchy flag behavior.

## Suggested Verification Commands

Run:

```bash
cargo fmt --all
cargo test -p egglsp
cargo test --lib lsp
```

Then, if feasible:

```bash
cargo test --all --workspace
```

If full workspace tests are skipped, record why in the implementation summary.

## Review Checklist

Before considering this patch complete:

- `semanticContext` passes include_call_hierarchy into `SemanticContextRequest`.
- `semanticContext` passes include_type_hierarchy into `SemanticContextRequest`.
- `securityContext` passes call-hierarchy intent into `SemanticContextRequest` when appropriate.
- Public packet adaptation still emits shared hierarchy data.
- Dead reverse adapter is removed or test-only.
- Tests cover the request propagation bug.
- No hunk/source navigation code is added yet.

## Expected Follow-Up

After this patch, start the hunk/source navigation phase. That work should consume `SemanticContextResponse` and add hunk-aware evidence on top rather than introducing another parallel LSP collection path.
