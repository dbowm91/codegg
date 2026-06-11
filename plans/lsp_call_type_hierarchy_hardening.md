# LSP Call and Type Hierarchy Hardening Plan

## Purpose

Tighten the call/type hierarchy implementation before moving into security-oriented semantic context packets.

The current pass broadly landed:

- `callHierarchy` and `typeHierarchy` direct operations.
- `HierarchyDirection` parsing.
- `egglsp` wrappers for prepare/incoming/outgoing/supertypes/subtypes.
- Compact hierarchy DTOs.
- Optional `semanticContext` `call_hierarchy` and `type_hierarchy` sections.
- Basic schema, validation, direction, and error serialization tests.

This hardening pass should align behavior with the intended contract, improve reliability with language servers, make truncation flags precise, and update docs.

## Current Issues

1. `semanticContext` silently ignores hierarchy flags when `line`/`column` are absent. The intended contract was to reject hierarchy sections without a full target position.
2. `egglsp` hierarchy prepare operations use `get_or_create_client` instead of `ensure_file_open_from_disk`, which may produce inconsistent position-sensitive behavior with some servers.
3. Hierarchy truncation flags are conservative but imprecise; exact cap hits are marked truncated even when no extra item was dropped.
4. `architecture/lsp.md` does not meaningfully document call/type hierarchy responsibilities, limitations, shallow traversal, or unsupported-server behavior.
5. `.opencode/skills/lsp/SKILL.md` and `AGENTS.md` may still omit the new hierarchy workflow.
6. Tests currently encode the weaker semanticContext behavior: hierarchy flags without position succeed and produce no sections.

## Non-Goals

Do not add recursive graph traversal.

Do not add security-specific hierarchy ranking.

Do not add graph visualization.

Do not expose raw LSP hierarchy objects.

Do not mutate files.

Do not execute commands.

Do not make hierarchy collection default-on in `semanticContext`.

## Phase 1 — Enforce SemanticContext Position Contract

Current behavior:

```text
semanticContext + include_call_hierarchy=true + no line/column => succeeds with None section
```

Change behavior to explicit validation:

```rust
let include_call_hierarchy = parsed.include_call_hierarchy.unwrap_or(false);
let include_type_hierarchy = parsed.include_type_hierarchy.unwrap_or(false);

if (include_call_hierarchy || include_type_hierarchy) && !has_position {
    return Err(ToolError::Execution(
        "semanticContext hierarchy sections require both line and column".to_string(),
    ));
}
```

This should run after the existing line/column pair validation, so partial position still returns:

```text
semanticContext requires both line and column when either is supplied
```

Acceptance criteria:

- hierarchy sections are not silently ignored;
- missing target position returns a clear validation error;
- existing behavior for semanticContext without hierarchy flags remains unchanged.

## Phase 2 — Update Tests for Position Contract

Replace the current permissive test:

```text
semanticContext_hierarchy_requires_line_column
```

The current test says hierarchy flags without position should not fail. Replace it with:

```rust
#[tokio::test]
async fn semanticContext_hierarchy_requires_line_column() {
    let tool = make_tool();
    let err = tool.execute(json!({
        "operation": "semanticContext",
        "file_path": "src/tool/mod.rs",
        "include_call_hierarchy": true,
        "include_type_hierarchy": true
    })).await.unwrap_err();
    assert!(matches!(err, ToolError::Execution(ref m)
        if m.contains("hierarchy sections require both line and column")));
}
```

Add separate partial-position tests if not already covered:

```text
semanticContext_hierarchy_rejects_line_without_column
semanticContext_hierarchy_rejects_column_without_line
```

Acceptance criteria:

- tests reflect the stricter contract;
- no test expects silent omission for requested hierarchy sections.

## Phase 3 — Ensure Documents Are Open Before Hierarchy Prepare

In `crates/egglsp/src/operations.rs`, change:

```rust
let (key, _root) = self.service.get_or_create_client(file_path).await?;
```

inside:

```text
prepare_call_hierarchy
prepare_type_hierarchy
```

to:

```rust
let (key, _root) = self.service.ensure_file_open_from_disk(file_path).await?;
```

Rationale: hierarchy prepare requests are position-sensitive and should operate against a document view known to the server, matching other document-aware preview/semantic paths.

Do not change incoming/outgoing/super/subtype follow-up requests unless required. Those requests operate from the prepared item URI and can continue resolving the relevant client from the item URI.

Acceptance criteria:

- prepare call hierarchy opens/syncs file from disk first;
- prepare type hierarchy opens/syncs file from disk first;
- no file writes are introduced;
- direct hierarchy operations remain read-only.

## Phase 4 — Make Truncation Flags Precise

Current code often uses:

```rust
let capped = items.into_iter().take(MAX).collect();
let truncated = capped.len() >= MAX;
```

This marks exact cap-sized responses as truncated even if nothing was dropped.

Change to raw length checks before `take`.

For call hierarchy prepare items:

```rust
let items_truncated = items.len() > MAX_HIERARCHY_ITEMS;
let item_summaries = items.iter().take(MAX_HIERARCHY_ITEMS) ...;
```

For incoming/outgoing calls:

```rust
let incoming_truncated = calls.len() > MAX_HIERARCHY_EDGES;
let incoming = calls.into_iter().take(MAX_HIERARCHY_EDGES) ...;
```

For ranges inside each call:

```rust
let ranges_truncated = call.from_ranges.len() > MAX_HIERARCHY_RANGES;
```

You can either:

1. keep a single summary `truncated` boolean that ORs all section/range truncation; or
2. add explicit limit fields later.

For this hardening pass, keep the single `truncated` field but compute it accurately:

```rust
let truncated = items_truncated || incoming_truncated || outgoing_truncated || ranges_truncated;
```

For type hierarchy:

```rust
let items_truncated = items.len() > MAX_HIERARCHY_ITEMS;
let supertypes_truncated = supertypes_raw.len() > MAX_HIERARCHY_ITEMS;
let subtypes_truncated = subtypes_raw.len() > MAX_HIERARCHY_ITEMS;
let truncated = items_truncated || supertypes_truncated || subtypes_truncated;
```

Acceptance criteria:

- exact cap-sized results are not marked truncated unless data was actually dropped;
- over-cap results are marked truncated;
- tests cover exact cap and over-cap behavior at pure helper level if feasible.

## Phase 5 — Consider a Small Pure Capping Helper

To make precise truncation easy to test, add helpers:

```rust
fn take_capped<T>(items: Vec<T>, max: usize) -> (Vec<T>, bool) {
    let truncated = items.len() > max;
    (items.into_iter().take(max).collect(), truncated)
}
```

For borrowed items:

```rust
fn iter_capped<'a, T>(items: &'a [T], max: usize) -> (impl Iterator<Item = &'a T>, bool)
```

Keep it simple. A `take_capped` helper is enough for owned call/type vectors.

Acceptance criteria:

- capping behavior is testable without an LSP server;
- hierarchy builders use the helper where practical;
- no complicated generic abstraction is introduced.

## Phase 6 — Harden Hierarchy Summary Tests

Add pure tests for capping/truncation if helpers are introduced:

```text
take_capped_exact_cap_not_truncated
take_capped_over_cap_truncated
```

Add operation-validation tests:

```text
callHierarchy_requires_column_when_line_present
typeHierarchy_requires_column_when_line_present
callHierarchy_invalid_direction_rejected
typeHierarchy_invalid_direction_rejected
```

Existing direction tests are good; keep them.

Add semanticContext contract tests:

```text
semanticContext_call_hierarchy_requires_position
semanticContext_type_hierarchy_requires_position
semanticContext_hierarchy_with_position_accepts_flags
```

The last test should use an existing repo file and may produce error fields if no server is available; it should assert that the request does not fail due to validation when line+column are supplied.

Acceptance criteria:

- stricter semanticContext hierarchy validation is tested;
- precise truncation behavior is tested;
- no default test requires a live language server unless already guarded.

## Phase 7 — Documentation Updates

Update:

```text
architecture/lsp.md
architecture/tool.md
.opencode/skills/lsp/SKILL.md
AGENTS.md if it documents LSP behavior
```

Add to `architecture/lsp.md` key responsibilities:

```markdown
- Shallow call/type hierarchy queries (`callHierarchy`, `typeHierarchy`) — read-only, bounded, non-recursive relationship summaries for the symbol at a target position.
```

Add a hierarchy section:

```markdown
### Hierarchy operations

`callHierarchy` and `typeHierarchy` are read-only code-intelligence operations. They require `file_path`, `line`, and `column`. Both operations default to `direction="both"`.

`callHierarchy` maps:
- `incoming` → callers of the target symbol
- `outgoing` → calls made by the target symbol

`typeHierarchy` maps:
- `incoming` → supertypes
- `outgoing` → subtypes

The first pass is shallow and non-recursive. It prepares the target hierarchy item and requests only the immediate incoming/outgoing or super/subtype relationships. Unsupported language servers may return empty sections or error fields.
```

Document `semanticContext` hierarchy flags:

```markdown
`semanticContext` can include hierarchy sections when `include_call_hierarchy=true` or `include_type_hierarchy=true`. These flags require `line` and `column`; requests without a target position are rejected rather than silently omitted.
```

Acceptance criteria:

- docs mention direct operations and semanticContext flags;
- docs state shallow/non-recursive limits;
- docs state unsupported-server behavior;
- docs state read-only/no-mutation invariant.

## Phase 8 — Safety/Regression Checks

Run these search checks:

```bash
rg "ensure_file_open_from_disk" crates/egglsp/src/operations.rs
rg "prepare_call_hierarchy|prepare_type_hierarchy" crates/egglsp/src/operations.rs src/tool/lsp.rs tests/lsp.rs
rg "executeCommand|workspace/applyEdit|completion|codeLens" src/tool/lsp.rs crates/egglsp/src/operations.rs
```

Expected:

- prepare hierarchy paths use `ensure_file_open_from_disk`;
- no command execution or workspace apply-edit path is introduced;
- no completion/codeLens exposure appears in model-facing `src/tool/lsp.rs`.

## Phase 9 — Validation Commands

Run:

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Targeted:

```bash
cargo test --test lsp hierarchy
cargo test --test lsp callHierarchy
cargo test --test lsp typeHierarchy
cargo test -p egglsp hierarchy
rg "callHierarchy|typeHierarchy|HierarchyDirection|include_call_hierarchy|include_type_hierarchy" src/tool/lsp.rs crates/egglsp/src/operations.rs tests/lsp.rs architecture/lsp.md architecture/tool.md .opencode/skills/lsp/SKILL.md AGENTS.md
```

Manual smoke:

```text
1. callHierarchy on a known function position.
2. typeHierarchy on a trait/type position.
3. semanticContext with hierarchy flags and line+column.
4. semanticContext with hierarchy flags and no line+column; confirm clear validation error.
5. Confirm no files changed.
```

## Done Criteria

This pass is complete when:

- semanticContext hierarchy flags require line+column;
- prepare hierarchy operations open/sync the document from disk before requests;
- truncation flags are precise rather than cap-equality based;
- tests reflect the stricter contract;
- docs describe call/type hierarchy behavior, limits, and unsupported-server handling;
- no mutation/command/completion/codeLens surface is introduced.

## Next Pass After This

After hardening, move to security-oriented semantic context packets.

That pass should prioritize bounded, review-friendly context around auth boundaries, unsafe code, IO/process/network calls, deserialization, dependency-sensitive files, and call paths relevant to security review.
