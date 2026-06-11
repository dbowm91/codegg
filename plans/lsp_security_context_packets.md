# LSP Security Context Packets Plan

## Purpose

Add a security-oriented semantic context operation that packages bounded, review-friendly code intelligence for the security agent.

This builds on the current LSP stack:

```text
semanticContext
  -> source excerpt
  -> diagnostics/symbols
  -> definitions/references
  -> overlay diagnostics
  -> source-action hints
  -> call/type hierarchy
```

The goal is to provide a compact `securityContext` operation that prioritizes high-risk code surfaces and relationships useful for security review, without adding mutation, command execution, or broad static-analysis claims.

This pass also folds in the remaining hierarchy cleanup:

1. Document call/type hierarchy behavior more completely.
2. Include per-call `from_ranges` truncation in hierarchy `truncated` accounting.

## Target Feature

Add a model-facing operation:

```json
{
  "operation": "securityContext",
  "file_path": "src/server/auth.rs",
  "line": 42,
  "column": 17,
  "radius": 80
}
```

Optional patch/overlay form:

```json
{
  "operation": "securityContext",
  "file_path": "src/server/auth.rs",
  "line": 42,
  "column": 17,
  "patch": "--- a/src/server/auth.rs\n+++ b/src/server/auth.rs\n@@ ..."
}
```

Recommended output shape:

```json
{
  "operation": "securityContext",
  "file_path": "src/server/auth.rs",
  "result_count": 18,
  "truncated": false,
  "results": {
    "file": "src/server/auth.rs",
    "target": { "line": 42, "column": 17 },
    "risk_markers": [...],
    "security_relevant_symbols": [...],
    "security_relevant_diagnostics": [...],
    "call_hierarchy": {...},
    "definitions": [...],
    "references": [...],
    "overlay": {...},
    "excerpt": {...},
    "limits": {...},
    "notes": [...]
  }
}
```

## Non-Goals

Do not claim full static analysis.

Do not implement taint analysis in this pass.

Do not execute code.

Do not run external security scanners.

Do not mutate files.

Do not expose raw LSP responses.

Do not add recursive hierarchy traversal beyond the existing shallow call/type hierarchy.

Do not make security judgments that require whole-program analysis. The output should be context, not a verdict.

## Security Context Philosophy

`securityContext` should answer:

```text
What code around this target is likely relevant to a security review?
```

It should not answer:

```text
Is this code definitely vulnerable?
```

The operation is a retrieval and prioritization layer for the security agent. It should surface context around risky APIs, trust boundaries, call relationships, diagnostics, and proposed changes while staying deterministic and bounded.

## Phase 0 — Fold-In Hierarchy Cleanup

### 0.1 Include range truncation in hierarchy `truncated`

Current hierarchy builders cap each `from_ranges` list with:

```rust
.take(MAX_HIERARCHY_RANGES)
```

but do not include range truncation in the summary `truncated` flag.

Fix in call hierarchy builder:

```rust
let mut ranges_truncated = false;
...
ranges_truncated |= call.from_ranges.len() > MAX_HIERARCHY_RANGES;
```

Then:

```rust
let truncated = items_truncated
    || incoming_raw_len > MAX_HIERARCHY_EDGES
    || outgoing_raw_len > MAX_HIERARCHY_EDGES
    || ranges_truncated;
```

Acceptance criteria:

- incoming `from_ranges` over cap sets `truncated=true`;
- outgoing `from_ranges` over cap sets `truncated=true`;
- exact cap does not set truncation;
- tests cover exact-cap and over-cap range behavior through a pure helper or builder-adjacent helper.

### 0.2 Add fuller hierarchy docs

Update:

```text
architecture/lsp.md
architecture/tool.md
.opencode/skills/lsp/SKILL.md
AGENTS.md if it documents LSP behavior
```

Add explicit hierarchy docs:

```markdown
### Hierarchy operations

`callHierarchy` and `typeHierarchy` are read-only code-intelligence operations. They require `file_path`, `line`, and `column`. Both default to `direction="both"`.

`callHierarchy` maps `incoming` to callers and `outgoing` to callees. `typeHierarchy` maps `incoming` to supertypes and `outgoing` to subtypes.

The first implementation is shallow and non-recursive. It prepares the target hierarchy item and requests immediate relationships only. Unsupported language servers may return empty sections or per-section error fields.

`semanticContext` can include hierarchy sections with `include_call_hierarchy=true` or `include_type_hierarchy=true`. Those flags require `line` and `column`; requests without a target position are rejected.
```

Acceptance criteria:

- hierarchy docs mention direct operations;
- docs mention semanticContext hierarchy flags;
- docs state read-only, bounded, shallow, non-recursive behavior;
- docs state unsupported-server behavior.

## Phase 1 — Add Security Context DTOs

Add compact DTOs in `src/tool/lsp.rs` or a small `src/tool/lsp_security.rs` if `lsp.rs` is getting too large.

Recommended first-pass DTOs:

```rust
#[derive(Serialize)]
struct SecurityContextPacket {
    file: String,
    target: Option<SemanticContextTarget>,
    excerpt: SourceExcerpt,
    risk_markers: Vec<SecurityRiskMarker>,
    security_relevant_symbols: Vec<SymbolSummary>,
    security_relevant_diagnostics: Vec<DiagnosticSummary>,
    definitions: Vec<LocationSummary>,
    references: Vec<LocationSummary>,
    call_hierarchy: Option<CallHierarchySummary>,
    overlay: Option<SemanticOverlaySummary>,
    notes: Vec<String>,
    limits: SecurityContextLimits,
}

#[derive(Serialize)]
struct SecurityRiskMarker {
    category: String,
    label: String,
    line: u32,
    column: u32,
    matched_text: String,
    rationale: String,
}

#[derive(Serialize)]
struct SecurityContextLimits {
    risk_markers_truncated: bool,
    diagnostics_truncated: bool,
    symbols_truncated: bool,
    references_truncated: bool,
    excerpt_truncated: bool,
}
```

Potential categories:

```text
auth
crypto
filesystem
network
process
unsafe
serialization
sql
path_traversal
secrets
ffi
concurrency
```

Acceptance criteria:

- DTOs are compact and stable;
- no raw LSP types leak into final output;
- risk markers include rationale but not a vulnerability verdict.

## Phase 2 — Add Input Fields and Schema

Extend `LspInput` with security-specific controls:

```rust
#[serde(default)]
include_call_hierarchy: Option<bool>, // already exists
#[serde(default)]
include_overlay: Option<bool>,        // already exists
#[serde(default)]
security_categories: Option<Vec<String>>,
#[serde(default)]
max_risk_markers: Option<usize>,
```

Add operation enum:

```text
securityContext
```

Schema descriptions:

```json
"security_categories": {
  "type": "array",
  "items": { "type": "string" },
  "description": "Optional risk marker categories to include in securityContext. Defaults to all supported categories."
}
```

```json
"max_risk_markers": {
  "type": "number",
  "description": "Maximum risk markers to return for securityContext. Default 80, max 200."
}
```

Defaults:

```rust
const DEFAULT_SECURITY_CONTEXT_RADIUS: u32 = 80;
const MAX_SECURITY_CONTEXT_RADIUS: u32 = 200;
const DEFAULT_MAX_RISK_MARKERS: usize = 80;
const MAX_RISK_MARKERS: usize = 200;
```

Acceptance criteria:

- `securityContext` appears in schema;
- the operation remains read-only;
- categories are optional and bounded.

## Phase 3 — Deterministic Risk Marker Scanner

Add a small deterministic scanner over the bounded excerpt text.

Do not shell out. Do not use regexes that are hard to audit unless already available and simple.

Recommended approach:

```rust
struct RiskPattern {
    category: &'static str,
    label: &'static str,
    needles: &'static [&'static str],
    rationale: &'static str,
}
```

Example first-pass patterns:

```rust
RiskPattern {
    category: "process",
    label: "process execution",
    needles: &["Command::new", "std::process::Command", "tokio::process::Command"],
    rationale: "process execution can cross a trust boundary and requires input validation",
}

RiskPattern {
    category: "unsafe",
    label: "unsafe Rust",
    needles: &["unsafe {", "unsafe fn", "unsafe impl"],
    rationale: "unsafe blocks bypass compiler memory-safety guarantees and deserve review",
}

RiskPattern {
    category: "filesystem",
    label: "filesystem access",
    needles: &["std::fs::", "tokio::fs::", "File::open", "OpenOptions"],
    rationale: "filesystem access may need path validation and permission review",
}

RiskPattern {
    category: "network",
    label: "network boundary",
    needles: &["TcpListener", "TcpStream", "UdpSocket", "axum::", "hyper::", "reqwest::"],
    rationale: "network-facing code often processes untrusted input",
}

RiskPattern {
    category: "serialization",
    label: "serialization/deserialization",
    needles: &["serde_json::from", "toml::from", "bincode::", "deserialize"],
    rationale: "deserialization can expand trust boundaries and parser attack surface",
}

RiskPattern {
    category: "sql",
    label: "database query",
    needles: &["sqlx::query", "rusqlite", "SELECT ", "INSERT ", "UPDATE ", "DELETE "],
    rationale: "database access should be reviewed for parameterization and authorization",
}

RiskPattern {
    category: "secrets",
    label: "secret material",
    needles: &["API_KEY", "SECRET", "TOKEN", "PASSWORD", "Authorization"],
    rationale: "secret-bearing code should avoid logging and accidental exposure",
}
```

Scanner behavior:

1. Iterate excerpt lines with real line numbers.
2. Case-sensitive for code identifiers; optionally case-insensitive for secret-like words.
3. Return at most capped marker count.
4. Do not include large matched text; cap matched text length.
5. Sort by line then category.

Acceptance criteria:

- scanner is deterministic and cheap;
- markers are bounded;
- categories can be filtered;
- tests cover matching, category filtering, caps, and line numbers.

## Phase 4 — Reuse Existing Semantic Context Pieces

`securityContext` should reuse existing helpers rather than reimplement the whole LSP path.

Required pieces:

1. `resolve_file`.
2. line/column validation behavior same as `semanticContext` when position is supplied.
3. `build_source_excerpt` with security radius defaults.
4. `DiagnosticsCollector::get_diagnostics_for_file`.
5. `document_symbols`.
6. `go_to_definition` and `find_references` when position exists.
7. `build_call_hierarchy_summary` when line/column exists.
8. `semantic_check_preview` when `content` or `patch` exists.

Recommended default behavior:

```text
include_call_hierarchy = true when line+column are provided
include_overlay = true when content or patch is provided
include_definitions/references = true when line+column are provided
```

Unlike `semanticContext`, `securityContext` can be opinionated: it should include call hierarchy by default when a target position is supplied because call relationships are often security-relevant.

Acceptance criteria:

- no duplicated overlay/patch mutation path;
- no disk writes;
- default security context is richer than semanticContext but still bounded.

## Phase 5 — Filter Symbols and Diagnostics for Security Relevance

Do not return all symbols/diagnostics by default unless already under cap.

Security-relevant symbol heuristics:

- Symbol name contains security-sensitive terms:
  - `auth`, `login`, `token`, `secret`, `password`, `session`, `cookie`, `jwt`, `permission`, `role`, `admin`, `encrypt`, `decrypt`, `sign`, `verify`, `parse`, `deserialize`, `upload`, `download`, `path`, `file`, `exec`, `command`, `shell`, `unsafe`.
- Symbol range overlaps or is near risk marker lines.
- Symbol contains target line.

Security-relevant diagnostic heuristics:

- severity error/warning first;
- diagnostics on or near risk marker lines;
- diagnostic message contains security-sensitive terms above;
- cap results.

Add constants:

```rust
const MAX_SECURITY_SYMBOLS: usize = 80;
const MAX_SECURITY_DIAGNOSTICS: usize = 80;
const SECURITY_NEARBY_LINE_RADIUS: u32 = 20;
```

Acceptance criteria:

- security packet returns prioritized symbols/diagnostics;
- caps and truncation flags are truthful;
- tests cover keyword filtering and marker-nearby filtering using pure helpers.

## Phase 6 — Implement `securityContext` Operation

Add branch:

```rust
"securityContext" => {
    let file = self.resolve_file(&parsed.file_path)?;
    if parsed.content.is_some() && parsed.patch.is_some() { ... }
    let target = ...; // same target handling as semanticContext
    let packet = self.build_security_context_packet(&ops, &file, &parsed).await?;
    serialize LspToolOutput { operation: "securityContext", ... }
}
```

Prefer helper extraction:

```rust
async fn build_security_context_packet(
    &self,
    ops: &crate::lsp::operations::LspOperations,
    file: &Path,
    parsed: &LspInput,
) -> Result<SecurityContextPacket, ToolError>
```

Acceptance criteria:

- `securityContext` is a read-only operation;
- target line/column is optional, but when one is supplied both are required;
- patch/content semantics match `semanticContext`;
- output is bounded and stable.

## Phase 7 — Tests

Default tests must be hermetic.

Risk marker tests:

```text
security_risk_scanner_detects_process_execution
security_risk_scanner_detects_unsafe
security_risk_scanner_detects_filesystem
security_risk_scanner_detects_network
security_risk_scanner_filters_categories
security_risk_scanner_caps_results
security_risk_scanner_preserves_line_numbers
```

Schema tests:

```text
lsp_schema_includes_securityContext
lsp_schema_includes_security_categories
lsp_schema_includes_max_risk_markers
securityContext_is_read_only
```

Validation tests:

```text
securityContext_requires_file_path
securityContext_rejects_line_without_column
securityContext_rejects_content_and_patch
securityContext_patch_does_not_write_disk
```

Packet tests:

```text
securityContext_returns_risk_markers_for_temp_file
securityContext_with_patch_does_not_mutate_disk
securityContext_result_count_includes_markers
```

Use temp files and disabled/no-op LSP where possible. It is acceptable for diagnostics/symbols to be empty in hermetic tests; marker scanning should still work from file content.

Acceptance criteria:

- no default test requires an external LSP server;
- scanner behavior is well covered;
- no-disk-write behavior is covered.

## Phase 8 — Documentation

Update:

```text
architecture/lsp.md
architecture/tool.md
.opencode/skills/lsp/SKILL.md
AGENTS.md if relevant
```

Document:

- `securityContext` is read-only and bounded;
- it provides deterministic risk markers, not vulnerability verdicts;
- it reuses LSP diagnostics/symbols/definitions/references/call hierarchy/overlay where available;
- it never writes files;
- risk categories are heuristics and can be filtered;
- it is intended as input to the security agent’s review workflow.

Recommended snippet:

```markdown
`securityContext` is a security-review context packet. It combines bounded source excerpts, deterministic risk markers, prioritized diagnostics/symbols, optional definition/reference/call hierarchy context, and optional overlay diagnostics for proposed content or a single-file patch. It is not a vulnerability scanner and does not mutate files.
```

Acceptance criteria:

- docs distinguish context retrieval from vulnerability detection;
- docs list available categories;
- docs preserve read-only/no-mutation boundary.

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
cargo test --test lsp securityContext
cargo test --test lsp security_context
cargo test --test lsp security_risk
rg "securityContext|SecurityContextPacket|SecurityRiskMarker|security_categories|max_risk_markers" src/tool/lsp.rs tests/lsp.rs architecture/lsp.md architecture/tool.md .opencode/skills/lsp/SKILL.md AGENTS.md
rg "workspace/applyEdit|executeCommand|std::fs::write|tokio::fs::write" src/tool/lsp.rs crates/egglsp/src/operations.rs
```

Manual smoke:

```text
1. Run securityContext on a file containing std::process::Command.
2. Confirm process risk marker appears.
3. Run securityContext on a file with unsafe block.
4. Confirm unsafe marker appears.
5. Run securityContext with security_categories=["process"] and confirm other categories are omitted.
6. Run securityContext with a patch and confirm disk file is unchanged.
7. Run securityContext with line+column and confirm call hierarchy is included when server supports it, or error fields are visible when unsupported.
```

## Done Criteria

This pass is complete when:

- hierarchy range truncation is counted correctly;
- hierarchy docs are complete;
- `securityContext` is exposed as read-only;
- risk marker scanning is deterministic, bounded, and tested;
- security diagnostics/symbol filtering is implemented and bounded;
- patch/content overlay behavior is no-disk-write and tested;
- docs clearly state that this is context gathering, not a vulnerability verdict.

## Follow-Up Passes

After this lands:

1. Add configurable security category presets for Rust/server/web/CLI code.
2. Add optional bounded recursive call expansion for security context only.
3. Add integration with dependency metadata and vulnerable dependency surfacing.
4. Add a dedicated security-agent prompt profile that consumes `securityContext` packets.
