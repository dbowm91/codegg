# LSP Security Context Bounded Call Expansion Plan

## Purpose

Add optional, bounded recursive call expansion to `securityContext` so the security agent can request a shallow call neighborhood around a target symbol.

The current stack is ready for this:

- `securityContext` is read-only, bounded, and preset-aware.
- Call hierarchy summaries already exist and are shallow/non-recursive.
- Presets and settings resolution are stable and tested.
- Output truncation and provenance are coherent.

This pass should add explicit, default-off expansion only. It must remain read-only, strictly capped, and resilient to cycles and unsupported LSP servers.

## Target Feature

Support input like:

```json
{
  "operation": "securityContext",
  "file_path": "src/server/auth.rs",
  "line": 42,
  "column": 17,
  "security_preset": "rust_server",
  "call_depth": 1,
  "max_call_nodes": 32
}
```

And for a slightly broader but still bounded review:

```json
{
  "operation": "securityContext",
  "file_path": "src/server/auth.rs",
  "line": 42,
  "column": 17,
  "call_depth": 2,
  "call_direction": "incoming",
  "max_call_nodes": 24
}
```

The output should add a compact call expansion section, distinct from the existing shallow `call_hierarchy` summary:

```json
"call_expansion": {
  "root": {...},
  "direction": "incoming",
  "depth": 2,
  "nodes": [...],
  "edges": [...],
  "truncated": false,
  "errors": [...]
}
```

## Non-Goals

Do not make expansion default-on.

Do not exceed depth 2 in this pass.

Do not add whole-program call graph analysis.

Do not add taint analysis.

Do not add vulnerability verdicts.

Do not mutate files.

Do not execute commands.

Do not add dependency/CVE metadata.

Do not expose raw LSP hierarchy items.

Do not request arbitrary code actions.

## Input Fields

Add to `LspInput`:

```rust
#[serde(default)]
call_depth: Option<u8>,
#[serde(default)]
max_call_nodes: Option<usize>,
#[serde(default)]
call_direction: Option<String>,
```

Schema:

```json
"call_depth": {
  "type": "number",
  "description": "Optional securityContext call expansion depth. Default 0/off. Max 2. Requires line+column."
}
```

```json
"max_call_nodes": {
  "type": "number",
  "description": "Maximum call expansion nodes for securityContext. Default 32, max 64."
}
```

```json
"call_direction": {
  "type": "string",
  "enum": ["incoming", "outgoing", "both"],
  "description": "Direction for securityContext call expansion. incoming=callers, outgoing=callees, both=both. Default both."
}
```

Constants:

```rust
const DEFAULT_CALL_EXPANSION_DEPTH: u8 = 0;
const MAX_CALL_EXPANSION_DEPTH: u8 = 2;
const DEFAULT_MAX_CALL_NODES: usize = 32;
const MAX_CALL_NODES: usize = 64;
const MAX_CALL_EDGES: usize = 128;
```

Acceptance criteria:

- schema exposes all fields;
- default behavior is unchanged (`call_depth=0` means no expansion);
- invalid direction and over-limit depth are rejected or clamped according to rules below.

## Validation Rules

Recommended behavior:

```text
call_depth omitted => 0/off
call_depth = 0 => no expansion
call_depth = 1 or 2 => expansion enabled
call_depth > 2 => ToolError::Execution with clear max-depth message
max_call_nodes omitted => 32
max_call_nodes > 64 => clamp to 64
call_direction omitted => both
call_direction invalid => ToolError::Execution
call_depth > 0 without line+column => ToolError::Execution
```

Why reject depth > 2 instead of clamp: expansion depth is semantic and expensive. Silent clamping can hide caller mistakes. Node caps can be safely clamped.

Acceptance criteria:

- depth over max returns explicit error;
- expansion requires target position;
- max nodes clamp;
- direction parsing reuses or mirrors `HierarchyDirection`.

## DTOs

Add compact DTOs in `src/tool/lsp.rs` or a small helper module if `lsp.rs` grows too much.

Recommended structures:

```rust
#[derive(Serialize)]
struct CallExpansionSummary {
    root: Option<CallExpansionNode>,
    direction: String,
    depth: u8,
    nodes: Vec<CallExpansionNode>,
    edges: Vec<CallExpansionEdge>,
    truncated: bool,
    errors: Vec<String>,
}

#[derive(Serialize, Clone)]
struct CallExpansionNode {
    id: String,
    name: String,
    kind: String,
    file: Option<String>,
    range: HierarchyRangeSummary,
    selection_range: HierarchyRangeSummary,
    detail: Option<String>,
    depth: u8,
}

#[derive(Serialize)]
struct CallExpansionEdge {
    from: String,
    to: String,
    direction: String,
    ranges: Vec<HierarchyRangeSummary>,
}
```

Node ID should be deterministic and stable enough for packet use:

```text
file_or_uri:name:selection_start_line:selection_start_column
```

Do not use random IDs.

Acceptance criteria:

- expansion output is compact;
- output uses summary DTOs, not raw LSP types;
- node IDs are deterministic;
- range lists are capped.

## Settings Integration

Extend `EffectiveSecurityContextSettings`:

```rust
call_depth: u8,
max_call_nodes: usize,
call_direction: HierarchyDirection,
```

Resolution:

1. Defaults:

```text
call_depth = 0
max_call_nodes = 32
call_direction = both
```

2. Presets do not currently enable recursion.

All presets keep:

```text
call_depth = 0
```

3. Explicit fields override.

4. Clamp max nodes, reject depth > 2.

Acceptance criteria:

- presets do not silently enable recursive expansion;
- expansion is only activated through explicit `call_depth > 0`;
- setting behavior is directly tested.

## Expansion Algorithm

Use existing LSP call hierarchy operations:

- `prepare_call_hierarchy` for root only;
- `incoming_calls` for callers;
- `outgoing_calls` for callees.

Expansion should be breadth-first and strictly capped.

Pseudo-code:

```rust
async fn build_call_expansion_summary(
    &self,
    ops: &LspOperations,
    file: &Path,
    line: u32,
    column: u32,
    direction: HierarchyDirection,
    max_depth: u8,
    max_nodes: usize,
) -> CallExpansionSummary {
    let root_items = ops.prepare_call_hierarchy(file, pos).await?;
    let Some(root) = root_items.first() else { return empty summary };

    queue.push((root, 0));
    seen.insert(node_id(root));

    while let Some((item, depth)) = queue.pop_front() {
        if depth >= max_depth { continue; }
        if nodes.len() >= max_nodes { truncated = true; break; }

        if direction includes incoming {
            for call in ops.incoming_calls(&item).await? { ... }
        }
        if direction includes outgoing {
            for call in ops.outgoing_calls(&item).await? { ... }
        }
    }
}
```

For incoming calls:

```text
caller -> current
```

For outgoing calls:

```text
current -> callee
```

Cycle handling:

```rust
if seen.insert(child_id.clone()) {
    queue.push_back((child_item, depth + 1));
}
```

Still add an edge for already-seen nodes, unless edge cap is hit.

Caps:

- `max_nodes` caps nodes.
- `MAX_CALL_EDGES` caps edges.
- `MAX_HIERARCHY_RANGES` caps ranges per edge.
- `truncated=true` when any cap drops data.

Acceptance criteria:

- BFS expansion only;
- depth limited to 2;
- cycles cannot loop forever;
- repeated nodes are deduped;
- edges can point to already-seen nodes;
- truncation reflects dropped nodes, edges, or ranges.

## Error Handling

Do not fail the whole `securityContext` packet because expansion fails.

Expansion errors should land in:

```rust
CallExpansionSummary.errors: Vec<String>
```

Examples:

```text
prepare_call_hierarchy: server returned method not supported
incoming_calls for <node-id>: request failed: ...
outgoing_calls for <node-id>: request failed: ...
```

If root prepare fails:

```text
root = None
nodes = []
edges = []
errors = [prepare error]
```

If a child request fails:

```text
keep already-collected nodes/edges
append error
continue other queued items if possible
```

Acceptance criteria:

- expansion failures are visible but nonfatal;
- packet still returns risk markers and normal context;
- structured success remains tied to restore errors, not expansion errors.

## SecurityContext Integration

Add to `SecurityContextPacket`:

```rust
call_expansion: Option<CallExpansionSummary>,
```

In `securityContext` branch:

```rust
let call_expansion = if settings.call_depth > 0 {
    Some(
        self.build_call_expansion_summary(
            &ops,
            &file,
            parsed.line.unwrap(),
            parsed.column.unwrap(),
            settings.call_direction,
            settings.call_depth,
            settings.max_call_nodes,
        ).await
    )
} else {
    None
};
```

Validation should already ensure line+column when `call_depth > 0`.

Update `result_count`:

```rust
+ call_expansion.nodes.len()
+ call_expansion.edges.len()
```

Update top-level truncation:

```rust
|| call_expansion.as_ref().is_some_and(|c| c.truncated)
```

Either add a `call_expansion_truncated` field to `SecurityContextLimits`, or rely on `call_expansion.truncated` plus top-level `truncated`.

Recommendation: add `call_expansion_truncated` to `SecurityContextLimits` so all truncation flags are centralized.

Acceptance criteria:

- default securityContext output unchanged except new nullable field if serialized;
- expansion only appears when requested;
- result_count and truncated include expansion.

## Tests

Keep tests hermetic where possible. Pure helper tests should cover most expansion logic without a live LSP.

### Settings tests

```text
security_context_settings_default_call_depth_zero
security_context_settings_call_depth_one_enabled
security_context_settings_call_depth_two_enabled
security_context_settings_call_depth_over_max_rejected
security_context_settings_max_call_nodes_clamps
security_context_settings_call_direction_defaults_both
security_context_settings_call_direction_rejects_invalid
security_context_settings_call_depth_requires_position
```

The last can live at operation validation level if settings does not know position intent cleanly.

### DTO/helper tests

```text
call_expansion_node_id_is_deterministic
call_expansion_dedupes_seen_nodes
call_expansion_preserves_edges_to_seen_nodes
call_expansion_marks_range_truncation
call_expansion_marks_edge_truncation
call_expansion_marks_node_truncation
```

If full pure expansion is too much, extract small helpers:

```rust
fn call_expansion_node_id(item: &CallHierarchyItem) -> String
fn push_capped_edge(...)
fn push_capped_node(...)
```

### Operation tests

```text
securityContext_call_depth_zero_omits_call_expansion
securityContext_call_depth_requires_line_column
securityContext_call_depth_over_max_rejected
securityContext_max_call_nodes_clamps
securityContext_call_direction_invalid_rejected
securityContext_schema_includes_call_expansion_inputs
```

Avoid tests that require a live language server. If a no-op LSP service causes expansion errors, assert only that the packet returns with `call_expansion.errors` when line+column are supplied and depth > 0, not that actual graph nodes exist.

Acceptance criteria:

- no default test depends on a live LSP server;
- validation and cap behavior is covered;
- pure cap/dedup helpers are covered.

## Documentation

Update:

```text
architecture/lsp.md
architecture/tool.md
.opencode/skills/lsp/SKILL.md
AGENTS.md if relevant
```

Add section:

```markdown
### Security call expansion

`securityContext` can optionally include a bounded call expansion with `call_depth`. The default is `0`, which disables recursive expansion. Supported depths are `1` and `2`; higher depths are rejected. Expansion is breadth-first, dedupes repeated nodes, preserves edges to already-seen nodes, and is capped by `max_call_nodes` and internal edge/range limits.

This is not whole-program analysis. It is a shallow LSP-backed neighborhood around the target symbol for review triage.
```

Document input fields:

```text
call_depth: 0/off by default, max 2
max_call_nodes: default 32, max 64
call_direction: incoming | outgoing | both, default both
```

Document read-only boundary:

```text
Call expansion only sends LSP hierarchy requests. It never writes files or executes code.
```

Acceptance criteria:

- docs describe defaults, caps, and non-goals;
- docs clearly say expansion is not whole-program analysis;
- docs preserve no-mutation/no-verdict contract.

## Validation Commands

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
cargo test -p codegg security_context_settings_call_depth
cargo test -p codegg securityContext_call_depth
cargo test -p codegg lsp_parameters_schema_snapshot
rg "call_depth|max_call_nodes|call_direction|CallExpansionSummary|call_expansion" src/tool tests architecture .opencode AGENTS.md
rg "workspace/applyEdit|executeCommand|std::fs::write|tokio::fs::write" src/tool/lsp.rs src/tool/lsp_security.rs crates/egglsp/src/operations.rs
```

Manual smoke:

```text
1. securityContext without call_depth: confirm call_expansion absent/null.
2. securityContext with call_depth=1 and line+column: confirm call_expansion section returns or contains nonfatal errors.
3. securityContext with call_depth=3: confirm clear validation error.
4. securityContext with call_depth=1 and no position: confirm validation error.
5. securityContext with max_call_nodes=999: confirm cap clamps to 64 and output can truncate.
```

## Done Criteria

This pass is complete when:

- `call_depth`, `max_call_nodes`, and `call_direction` are schema-exposed;
- expansion is default-off;
- depth > 2 is rejected;
- expansion requires line+column;
- expansion is BFS, deduped, and capped;
- truncation includes node, edge, and range caps;
- errors are nonfatal and visible in `call_expansion.errors`;
- result_count and top-level truncated include expansion;
- tests cover settings, validation, caps, and helpers;
- docs explain limits and read-only behavior.

## Follow-Up Passes

After this lands:

1. Cleanup/hardening pass for call expansion if needed.
2. Optional dependency metadata context for manifests/lockfiles.
3. Optional security-agent prompt profile consuming `securityContext` packets.
4. Optional cached graph snippets if LSP expansion proves expensive.
