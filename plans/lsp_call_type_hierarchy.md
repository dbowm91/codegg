# LSP Call and Type Hierarchy Plan

## Purpose

Add read-only call hierarchy and type hierarchy support to Codegg's LSP layer.

The current LSP stack now provides stable preview/context primitives:

```text
sourceActionPreview
  -> allowlisted preview-only source actions

semanticCheckPreview(content | patch)
  -> temporary overlay diagnostics/symbols without disk writes

semanticContext
  -> bounded source excerpt
  -> diagnostics/symbols
  -> definitions/references
  -> optional overlay diagnostics
  -> optional source-action hints
```

This pass should add compact call/type relationship summaries for deeper code understanding, review, and security workflows.

The implementation should expose explicit read-only operations first, then optionally wire them into `semanticContext` behind opt-in flags.

## Target Features

Add model-facing operations:

```json
{
  "operation": "callHierarchy",
  "file_path": "src/main.rs",
  "line": 42,
  "column": 17,
  "direction": "both"
}
```

```json
{
  "operation": "typeHierarchy",
  "file_path": "src/main.rs",
  "line": 42,
  "column": 17,
  "direction": "both"
}
```

Add optional semantic-context sections:

```json
{
  "operation": "semanticContext",
  "file_path": "src/main.rs",
  "line": 42,
  "column": 17,
  "include_call_hierarchy": true,
  "include_type_hierarchy": true
}
```

Recommended output summary shape:

```json
{
  "item": {
    "name": "handle_request",
    "kind": "function",
    "file": "src/server.rs",
    "range": { "start_line": 12, "start_column": 1, "end_line": 40, "end_column": 2 },
    "selection_range": { "start_line": 12, "start_column": 4, "end_line": 12, "end_column": 18 }
  },
  "incoming": [...],
  "outgoing": [...],
  "supertypes": [...],
  "subtypes": [...],
  "errors": {...},
  "truncated": false
}
```

## Non-Goals

Do not expose raw LSP hierarchy response objects.

Do not mutate files.

Do not execute commands.

Do not add completion/code lens.

Do not add graph visualization in this pass.

Do not recursively traverse arbitrarily deep call/type graphs.

Do not make hierarchy collection default-on inside `semanticContext`.

Do not require all language servers to support hierarchy. Unsupported capability should be a normal, nonfatal result.

## Current State Summary

Relevant files:

```text
crates/egglsp/src/operations.rs
crates/egglsp/src/client.rs
crates/egglsp/src/service.rs
src/tool/lsp.rs
tests/lsp.rs
architecture/lsp.md
architecture/tool.md
```

Useful existing pieces:

- `LspOperations` already wraps higher-level request flows.
- `goToDefinition` and `findReferences` already summarize locations.
- `semanticContext` already validates `file_path`, line/column pairing, and exposes opt-in sections.
- `uri_to_path`, `symbol_kind_to_string`, and `LocationSummary` already exist in `src/tool/lsp.rs`.
- `lsp` remains `ToolCategory::ReadOnly`.

## Design Rule

Hierarchy operations should be shallow, bounded, and explicit.

First pass should collect only the hierarchy level directly returned by the language server:

```text
prepare item at target position
  -> incoming calls OR outgoing calls
  -> supertypes OR subtypes
```

Do not recursively fan out into graph traversal. Recursion can be added later with hard depth/node caps.

## Phase 1 — Add Direction Enum

Add a small direction parser at the tool layer and/or `egglsp` layer:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HierarchyDirection {
    Incoming,
    Outgoing,
    Both,
}

impl HierarchyDirection {
    pub fn parse(input: Option<&str>) -> Result<Self, ToolError> {
        match input.unwrap_or("both") {
            "incoming" => Ok(Self::Incoming),
            "outgoing" => Ok(Self::Outgoing),
            "both" => Ok(Self::Both),
            other => Err(ToolError::Execution(format!("unsupported hierarchy direction: {other}"))),
        }
    }
}
```

For type hierarchy, use the same enum but map:

```text
incoming  -> supertypes
outgoing  -> subtypes
both      -> supertypes + subtypes
```

Alternatively use strings directly in the tool layer, but a typed enum is cleaner.

Acceptance criteria:

- default direction is `both`;
- invalid direction returns a clear error;
- tests cover direction parsing.

## Phase 2 — Add `egglsp` Operations

Add to `crates/egglsp/src/operations.rs`:

```rust
pub async fn prepare_call_hierarchy(
    &self,
    file_path: &Path,
    line: u32,
    column: u32,
) -> Result<Vec<CallHierarchyItem>, LspError>;

pub async fn incoming_calls(
    &self,
    item: CallHierarchyItem,
) -> Result<Vec<CallHierarchyIncomingCall>, LspError>;

pub async fn outgoing_calls(
    &self,
    item: CallHierarchyItem,
) -> Result<Vec<CallHierarchyOutgoingCall>, LspError>;
```

And:

```rust
pub async fn prepare_type_hierarchy(
    &self,
    file_path: &Path,
    line: u32,
    column: u32,
) -> Result<Vec<TypeHierarchyItem>, LspError>;

pub async fn supertypes(
    &self,
    item: TypeHierarchyItem,
) -> Result<Vec<TypeHierarchyItem>, LspError>;

pub async fn subtypes(
    &self,
    item: TypeHierarchyItem,
) -> Result<Vec<TypeHierarchyItem>, LspError>;
```

LSP methods:

```text
textDocument/prepareCallHierarchy
callHierarchy/incomingCalls
callHierarchy/outgoingCalls
textDocument/prepareTypeHierarchy
typeHierarchy/supertypes
typeHierarchy/subtypes
```

Implementation notes:

1. Call `ensure_file_open_from_disk(file_path)` before prepare.
2. Build a `TextDocumentPositionParams` with file URI and position.
3. If prepare returns `null`, return an empty vector.
4. If server returns method-not-found or unsupported, return a clear `LspError` that wrapper can convert into a nonfatal error field.
5. Keep raw `lsp_types` internal to `egglsp` or summarize at tool layer.

Acceptance criteria:

- operations compile with existing `lsp_types` version;
- null prepare responses become empty results;
- unsupported servers fail clearly but not catastrophically.

## Phase 3 — Add Compact Tool DTOs

In `src/tool/lsp.rs`, add compact hierarchy summaries:

```rust
#[derive(Serialize)]
struct HierarchyRangeSummary {
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
}

#[derive(Serialize)]
struct HierarchyItemSummary {
    name: String,
    kind: String,
    file: Option<String>,
    range: HierarchyRangeSummary,
    selection_range: HierarchyRangeSummary,
    detail: Option<String>,
}

#[derive(Serialize)]
struct IncomingCallSummary {
    from: HierarchyItemSummary,
    from_ranges: Vec<HierarchyRangeSummary>,
}

#[derive(Serialize)]
struct OutgoingCallSummary {
    to: HierarchyItemSummary,
    from_ranges: Vec<HierarchyRangeSummary>,
}

#[derive(Serialize)]
struct CallHierarchySummary {
    items: Vec<HierarchyItemSummary>,
    incoming: Vec<IncomingCallSummary>,
    outgoing: Vec<OutgoingCallSummary>,
    prepare_error: Option<String>,
    incoming_error: Option<String>,
    outgoing_error: Option<String>,
    truncated: bool,
}

#[derive(Serialize)]
struct TypeHierarchySummary {
    items: Vec<HierarchyItemSummary>,
    supertypes: Vec<HierarchyItemSummary>,
    subtypes: Vec<HierarchyItemSummary>,
    prepare_error: Option<String>,
    supertypes_error: Option<String>,
    subtypes_error: Option<String>,
    truncated: bool,
}
```

Caps:

```rust
const MAX_HIERARCHY_ITEMS: usize = 32;
const MAX_HIERARCHY_EDGES: usize = 128;
const MAX_HIERARCHY_RANGES: usize = 32;
```

Acceptance criteria:

- hierarchy output is compact and bounded;
- URI conversion uses existing `uri_to_path` logic;
- range fields are 1-indexed like other tool outputs;
- errors are per-section and nonfatal where possible.

## Phase 4 — Add Tool Input Fields and Schema

Extend `LspInput`:

```rust
#[serde(default)]
direction: Option<String>,

#[serde(default)]
include_call_hierarchy: Option<bool>,

#[serde(default)]
include_type_hierarchy: Option<bool>,
```

Update operation enum:

```text
callHierarchy
typeHierarchy
```

Schema descriptions:

```json
"direction": {
  "type": "string",
  "enum": ["incoming", "outgoing", "both"],
  "description": "Hierarchy direction for callHierarchy/typeHierarchy. Defaults to both. For typeHierarchy, incoming means supertypes and outgoing means subtypes."
}
```

```json
"include_call_hierarchy": {
  "type": "boolean",
  "description": "Include call hierarchy section in semanticContext. Requires line+column. Default false."
}
```

```json
"include_type_hierarchy": {
  "type": "boolean",
  "description": "Include type hierarchy section in semanticContext. Requires line+column. Default false."
}
```

Acceptance criteria:

- schema lists both direct operations;
- semanticContext flags are documented;
- `lsp` remains read-only.

## Phase 5 — Implement Direct `callHierarchy` Operation

Add branch in `execute`:

```rust
"callHierarchy" => {
    let file = self.resolve_file(&parsed.file_path)?;
    let (line, col) = self.require_line_col(&parsed.line, &parsed.column)?;
    let direction = HierarchyDirection::parse(parsed.direction.as_deref())?;
    let summary = self.build_call_hierarchy_summary(&ops, &file, line, col, direction).await;
    serialize LspToolOutput { operation: "callHierarchy", ... }
}
```

Behavior:

1. Prepare call hierarchy at target.
2. If no items, return empty `items`, no incoming/outgoing, no error.
3. Use only first prepared item for incoming/outgoing by default, or cap prepared items and use first as primary. Document this.
4. If direction includes incoming, request incoming calls.
5. If direction includes outgoing, request outgoing calls.
6. Preserve errors in `incoming_error`/`outgoing_error` without failing whole summary if prepare succeeded.

Acceptance criteria:

- direct operation works with line+column;
- no recursion;
- errors are visible;
- output is bounded.

## Phase 6 — Implement Direct `typeHierarchy` Operation

Add branch:

```rust
"typeHierarchy" => {
    let file = self.resolve_file(&parsed.file_path)?;
    let (line, col) = self.require_line_col(&parsed.line, &parsed.column)?;
    let direction = HierarchyDirection::parse(parsed.direction.as_deref())?;
    let summary = self.build_type_hierarchy_summary(&ops, &file, line, col, direction).await;
    serialize LspToolOutput { operation: "typeHierarchy", ... }
}
```

Behavior:

1. Prepare type hierarchy at target.
2. If no items, return empty results.
3. Use first prepared item as primary for supertypes/subtypes.
4. If direction includes incoming, request supertypes.
5. If direction includes outgoing, request subtypes.
6. Preserve per-section errors.

Acceptance criteria:

- direct operation works with line+column;
- unsupported servers report error fields;
- output is bounded and compact.

## Phase 7 — Wire Optional Sections into `semanticContext`

Add to `SemanticContextPacket`:

```rust
call_hierarchy: Option<CallHierarchySummary>,
type_hierarchy: Option<TypeHierarchySummary>,
```

Inside semanticContext:

```rust
let include_call_hierarchy = parsed.include_call_hierarchy.unwrap_or(false);
let include_type_hierarchy = parsed.include_type_hierarchy.unwrap_or(false);
```

If either is true and no full line+column target exists, return:

```text
semanticContext hierarchy sections require both line and column
```

Then:

```rust
let call_hierarchy = if include_call_hierarchy {
    Some(self.build_call_hierarchy_summary(&ops, &file, line, col, HierarchyDirection::Both).await)
} else { None };
```

Same for type hierarchy.

Recommended first pass: semanticContext hierarchy flags always use `both`; direct operations allow `direction`.

Acceptance criteria:

- hierarchy sections are opt-in;
- semanticContext remains bounded;
- no extra hierarchy requests occur unless flags are true;
- hierarchy errors do not fail the whole context packet unless input validation fails.

## Phase 8 — Tests

Default tests should remain hermetic where possible.

Pure tests:

```text
hierarchy_direction_defaults_to_both
hierarchy_direction_parses_incoming_outgoing_both
hierarchy_direction_rejects_invalid
hierarchy_item_summary_converts_uri_and_ranges
incoming_call_summary_caps_ranges
type_hierarchy_summary_serializes_errors
call_hierarchy_summary_serializes_errors
```

Schema tests:

```text
lsp_schema_includes_callHierarchy_and_typeHierarchy
lsp_schema_includes_direction
lsp_schema_includes_hierarchy_context_flags
callHierarchy_requires_file_path_line_column
typeHierarchy_requires_file_path_line_column
semanticContext_hierarchy_requires_line_column
```

Optional integration tests:

```bash
CODEGG_LSP_INTEGRATION=1 cargo test --test lsp callHierarchy_real_lsp -- --nocapture
CODEGG_LSP_INTEGRATION=1 cargo test --test lsp typeHierarchy_real_lsp -- --nocapture
```

Integration tests must skip cleanly when the server lacks hierarchy support.

Acceptance criteria:

- default tests do not require an external LSP server;
- conversion/capping/error serialization are covered;
- validation paths are covered.

## Phase 9 — Documentation

Update:

```text
architecture/lsp.md
architecture/tool.md
.opencode/skills/lsp/SKILL.md
AGENTS.md if it documents LSP operation behavior
```

Document:

- `callHierarchy` and `typeHierarchy` are read-only;
- output is shallow and bounded;
- default direction is `both`;
- type hierarchy maps incoming to supertypes and outgoing to subtypes;
- unsupported LSP servers may return empty results or error fields;
- optional semanticContext hierarchy sections require line+column;
- no recursion in first pass.

Acceptance criteria:

- docs match schema and DTOs;
- no claim of mutation or command execution;
- limitations are explicit.

## Phase 10 — Validation Commands

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
rg "callHierarchy|typeHierarchy|prepareCallHierarchy|prepareTypeHierarchy|incomingCalls|outgoingCalls|supertypes|subtypes" crates/egglsp src/tool/lsp.rs tests architecture .opencode AGENTS.md
rg "completion|codeLens|executeCommand" src/tool/lsp.rs crates/egglsp/src/operations.rs
```

Manual smoke:

```text
1. Run callHierarchy on a known function position.
2. Confirm incoming/outgoing summaries are bounded and path-normalized.
3. Run typeHierarchy on a type/interface/trait position.
4. Confirm unsupported server behavior is visible and nonfatal.
5. Run semanticContext with include_call_hierarchy=true and include_type_hierarchy=true.
6. Confirm no files are changed.
```

## Done Criteria

This pass is complete when:

- direct `callHierarchy` operation exists;
- direct `typeHierarchy` operation exists;
- optional semanticContext hierarchy sections exist;
- output is compact, bounded, and read-only;
- unsupported server behavior is visible through error fields;
- default tests cover parsing, schema, DTO conversion, and validation;
- docs describe hierarchy support and limits.

## Follow-Up Passes

After this lands:

1. Add security-oriented semantic context packets that prioritize call paths through auth, IO, unsafe code, deserialization, shell/process execution, network entrypoints, and dependency-sensitive files.
2. Add optional bounded recursive hierarchy expansion with strict depth/node caps.
3. Add a context-ranking pass to choose which semantic sections fit into model context for edit/review/security agents.
