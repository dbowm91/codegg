# LSP Security Context Call Expansion Hardening Plan

## Purpose

Tighten the first `securityContext` call-expansion implementation before building any larger security-agent workflow on top of it.

The first feature pass landed the main shape:

- `call_depth`, `max_call_nodes`, and `call_direction` inputs.
- `CallExpansionSummary`, `CallExpansionNode`, and `CallExpansionEdge` DTOs.
- Default-off expansion.
- Depth max enforcement.
- Position validation.
- BFS traversal over LSP call hierarchy.
- Cycle protection via node dedupe.
- Nonfatal expansion errors.
- Docs and basic tests.

This pass should focus on precise truncation, helper clarity, and hermetic tests. Do not expand feature surface.

## Current Issues

1. Edge cap truncation is imprecise. The builder checks:

```rust
if edges.len() < MAX_CALL_EDGES {
    edges.push(...);
}
```

but does not set `truncated = true` when an edge is dropped after the edge cap is hit.

2. Range cap truncation is imprecise. The builder does:

```rust
call.from_ranges.iter().take(MAX_HIERARCHY_RANGES)
```

but does not compare the raw range count with `MAX_HIERARCHY_RANGES`, so range drops are not reflected in `call_expansion.truncated`.

3. Node cap behavior is implemented, but there are no focused tests for node truncation semantics.

4. Dedupe/cycle behavior is implemented with `seen`, but pure tests do not yet prove repeated nodes are deduped while preserving edges to already-seen nodes.

5. The expansion code is embedded directly in `build_call_expansion_summary`; small helpers would make truncation/cycle semantics more testable.

## Non-Goals

Do not raise max depth.

Do not enable expansion by default.

Do not add new presets.

Do not add dependency metadata.

Do not add taint analysis.

Do not add whole-program graph analysis.

Do not mutate files.

Do not execute commands.

Do not change output schema unless necessary for existing truncation flags.

## Phase 1 — Add Explicit Cap Helpers

Extract small helper functions in `src/tool/lsp.rs` near the call-expansion helpers.

### Range conversion helper

```rust
fn capped_call_ranges(
    ranges: &[crate::lsp::lsp_types::Range],
) -> (Vec<HierarchyRangeSummary>, bool) {
    let truncated = ranges.len() > MAX_HIERARCHY_RANGES;
    let capped = ranges
        .iter()
        .take(MAX_HIERARCHY_RANGES)
        .map(|r| Self::convert_lsp_range(*r))
        .collect();
    (capped, truncated)
}
```

### Edge push helper

```rust
fn push_call_expansion_edge(
    edges: &mut Vec<CallExpansionEdge>,
    edge: CallExpansionEdge,
) -> bool {
    if edges.len() >= MAX_CALL_EDGES {
        return true; // truncated
    }
    edges.push(edge);
    false
}
```

### Node push helper

```rust
fn push_call_expansion_node(
    nodes: &mut Vec<CallExpansionNode>,
    node: CallExpansionNode,
    max_nodes: usize,
) -> bool {
    if nodes.len() >= max_nodes {
        return true; // truncated
    }
    nodes.push(node);
    false
}
```

If helper signatures need to differ for borrow-checking, keep the same semantics.

Acceptance criteria:

- range cap returns a truncation bool;
- edge cap returns a truncation bool;
- node cap returns a truncation bool;
- helpers are covered by pure tests.

## Phase 2 — Fix Edge and Range Truncation Accounting

In both incoming and outgoing expansion branches:

```rust
let (ranges, ranges_truncated) = Self::capped_call_ranges(&call.from_ranges);
truncated |= ranges_truncated;
```

Then:

```rust
let edge_truncated = Self::push_call_expansion_edge(&mut edges, edge);
truncated |= edge_truncated;
```

If an edge is dropped due to edge cap, still continue traversal if node cap allows. Edge cap should not necessarily abort node discovery unless this keeps implementation simpler. The key requirement is: dropped data must set `truncated = true`.

For node cap:

```rust
if seen.insert(child_id.clone()) {
    let node = ...;
    let node_truncated = Self::push_call_expansion_node(&mut nodes, node, max_nodes);
    truncated |= node_truncated;
    if !node_truncated {
        queue.push_back((child_item, child_depth));
    }
}
```

Acceptance criteria:

- dropped edges set `truncated=true`;
- dropped ranges set `truncated=true`;
- dropped nodes set `truncated=true`;
- cap behavior is deterministic and documented.

## Phase 3 — Clarify Cap-Control Flow

Current behavior may skip outgoing calls after incoming calls fill the node cap. This is acceptable, but make it explicit.

Preferred control flow:

```rust
while let Some((item, depth)) = queue.pop_front() {
    if depth >= max_depth { continue; }

    if direction includes incoming {
        expand incoming; // helpers set truncation
    }
    if direction includes outgoing {
        expand outgoing; // helpers set truncation
    }

    if nodes.len() >= max_nodes && queue_is_not_empty_or_more_children_seen {
        truncated = true;
        // continue draining? probably not needed
    }
}
```

Simpler acceptable approach:

- if `nodes.len() >= max_nodes`, stop queuing new nodes but still allow edges to already-seen nodes until edge cap;
- set `truncated=true` when any new unseen child cannot be added.

Do not overcomplicate. The important rule is that cap-driven omissions must be visible.

Acceptance criteria:

- cap behavior is understandable from code;
- no silent data drops;
- no infinite loop possible.

## Phase 4 — Add Pure Helper Tests

Add tests in `src/tool/lsp.rs` near existing call-expansion tests.

Recommended tests:

```text
call_expansion_capped_ranges_exact_cap_not_truncated
call_expansion_capped_ranges_over_cap_truncated
call_expansion_push_edge_exact_cap_not_truncated
call_expansion_push_edge_over_cap_truncated
call_expansion_push_node_exact_cap_not_truncated
call_expansion_push_node_over_cap_truncated
```

Use synthetic `Range` and simple `CallExpansionEdge`/`CallExpansionNode` builders.

Acceptance criteria:

- exact cap does not mark truncated;
- over cap does mark truncated;
- helpers are hermetic.

## Phase 5 — Add Expansion Semantics Tests

Because full async expansion requires LSP behavior, prefer pure tests for helper semantics and one or two operation-level validation tests.

Recommended pure tests:

```text
call_expansion_node_id_is_stable_for_same_item
call_expansion_node_id_differs_by_selection_position
call_expansion_edge_to_seen_node_can_be_preserved
```

For edge-to-seen semantics, test the helper behavior directly if extracting a small function is reasonable. If not, document that full dedupe semantics are covered by code review and leave integration tests for a fake LSP future pass.

Recommended operation tests:

```text
securityContext_call_depth_zero_omits_call_expansion // already exists; keep
securityContext_call_depth_requires_line_column // already exists; keep
securityContext_call_depth_over_max_rejected // already exists; keep
securityContext_call_direction_invalid_rejected // already exists; keep
securityContext_max_call_nodes_clamps_in_settings
securityContext_call_expansion_truncated_limit_field_present
```

The last can be structural: assert `results.limits.call_expansion_truncated` exists even when false.

Acceptance criteria:

- no live LSP server dependency is introduced beyond already-existing behavior;
- cap helpers are tested directly;
- output structure is pinned.

## Phase 6 — Tighten Result Count and Truncation Tests

Add tests that directly exercise output accounting where possible.

For no expansion:

```rust
assert!(v["results"]["call_expansion"].is_null());
assert_eq!(v["results"]["limits"]["call_expansion_truncated"], false);
```

For depth=1 with unavailable LSP:

```rust
assert!(!v["results"]["call_expansion"].is_null());
assert!(v["results"]["call_expansion"].get("errors").is_some());
```

Do not assert actual graph nodes unless a deterministic fake service exists.

Acceptance criteria:

- output structure is stable;
- expansion errors remain nonfatal;
- top-level `truncated` includes `call_expansion_truncated`.

## Phase 7 — Documentation Touch-Up

Update `architecture/lsp.md` only if behavior changes from the first feature pass.

Make cap semantics explicit:

```markdown
`call_expansion.truncated` is true when nodes, edges, or per-edge ranges are dropped due to configured or internal caps.
```

Add:

```markdown
When caps are reached, expansion prefers returning a partial graph with `truncated=true` rather than failing the entire packet.
```

Acceptance criteria:

- docs match implementation;
- docs state truncation means partial graph;
- docs preserve read-only/no-verdict contract.

## Phase 8 — Validation Commands

Run:

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Targeted:

```bash
cargo test -p codegg call_expansion
cargo test -p codegg securityContext_call_depth
cargo test -p codegg security_context_settings_call_depth
cargo test -p codegg lsp_parameters_schema_snapshot
rg "capped_call_ranges|push_call_expansion_edge|push_call_expansion_node|call_expansion_truncated|MAX_CALL_EDGES|MAX_HIERARCHY_RANGES" src/tool/lsp.rs tests architecture/lsp.md
rg "workspace/applyEdit|executeCommand|std::fs::write|tokio::fs::write" src/tool/lsp.rs src/tool/lsp_security.rs crates/egglsp/src/operations.rs
```

## Done Criteria

This hardening pass is complete when:

- edge cap drops set `call_expansion.truncated=true`;
- range cap drops set `call_expansion.truncated=true`;
- node cap drops set `call_expansion.truncated=true`;
- cap helpers exist and are tested;
- exact-cap and over-cap behavior is tested;
- output structure tests cover `call_expansion_truncated`;
- docs clarify partial graph/truncation semantics;
- no feature expansion, mutation, command execution, or vulnerability verdict behavior is introduced.

## Next Pass After This

After call-expansion hardening, move to a security-agent workflow plan that consumes `securityContext` packets:

- changed-hunk targeting;
- preset selection by file/use-case;
- deterministic preflight checks;
- evidence-based findings format;
- clear separation between risk markers and confirmed findings.
