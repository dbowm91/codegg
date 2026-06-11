# LSP Security Context Docs and Output Polish Plan

## Purpose

Finish the first `securityContext` implementation with a narrow documentation and output-consistency pass.

The current implementation is structurally solid:

- `securityContext` is schema-exposed and read-only.
- Risk scanning is extracted to `src/tool/lsp_security.rs`.
- Marker, diagnostic, and symbol truncation are precise.
- Diagnostics and symbols are filtered before capping.
- Nonfatal LSP subrequest failures are surfaced in packet notes.
- No-disk-write patch behavior is covered.

This pass should avoid feature expansion. It should only polish documentation and make top-level output metadata reflect nested limit flags.

## Current Issues

1. `securityContext` top-level `truncated` is still always `false`, even when `results.limits.*_truncated` contains true.
2. `ToolProvenance.truncated` is still always `false` in structured execution, even when the output is truncated.
3. `architecture/lsp.md` has key-responsibility bullets for `securityContext` and hierarchy, but the detailed docs still need a full contract section.
4. `architecture/tool.md`, `.opencode/skills/lsp/SKILL.md`, and possibly `AGENTS.md` may not yet explain the security context packet contract.
5. Schema descriptions for `content`, `patch`, and `radius` still primarily mention `semanticContext`; they should mention `securityContext` where applicable.

## Non-Goals

Do not add security presets.

Do not add recursive call expansion.

Do not change risk marker categories.

Do not change scanner matching behavior.

Do not add dependency/CVE metadata.

Do not change the output DTO schema except top-level `truncated` consistency.

Do not mutate files or add new command execution paths.

## Phase 1 — Add SecurityContext Top-Level Truncation

Current `securityContext` output does this:

```rust
let output = LspToolOutput {
    operation: "securityContext".to_string(),
    file_path: file_path_str,
    result_count,
    truncated: false,
    results: packet,
};
```

Change to compute a packet-level truncation bool:

```rust
let truncated = risk_markers_truncated
    || diagnostics_truncated
    || symbols_truncated
    || refs_truncated
    || excerpt_truncated;
```

Then:

```rust
let output = LspToolOutput {
    operation: "securityContext".to_string(),
    file_path: file_path_str,
    result_count,
    truncated,
    results: packet,
};
```

If overlay diagnostic truncation becomes part of `SecurityContextLimits` later, include it in this OR. For this pass, do not change the security packet schema unless already necessary.

Acceptance criteria:

- top-level `truncated` is false when all security limits are false;
- top-level `truncated` is true when any security limit is true;
- existing nested `results.limits` fields remain unchanged.

## Phase 2 — Reflect Truncation in Structured Provenance

`execute_structured` currently sets:

```rust
truncated: false,
```

for all LSP operations.

Update structured execution to parse the top-level `truncated` field from the JSON output:

```rust
let output_value = serde_json::from_str::<serde_json::Value>(&output).ok();
let truncated = output_value
    .as_ref()
    .and_then(|v| v.get("truncated"))
    .and_then(|v| v.as_bool())
    .unwrap_or(false);
```

Then set:

```rust
truncated: Some(truncated),
```

or whatever the current `ToolProvenance` field type requires.

Keep the existing restore-error success handling:

```rust
/results/restore_error
/results/overlay/restore_error
```

Acceptance criteria:

- structured provenance uses top-level truncation;
- restore-error success semantics remain unchanged;
- malformed JSON still falls back safely.

## Phase 3 — Update Schema Descriptions

Update `parameters()` descriptions so shared inputs mention `securityContext` where relevant.

Recommended updates:

```rust
"content": {
    "description": "Proposed full file content for semanticCheckPreview, semanticContext overlay, or securityContext overlay. Mutually exclusive with patch."
}
```

```rust
"patch": {
    "description": "Single-file unified diff patch to apply in memory for semanticCheckPreview, semanticContext overlay, or securityContext overlay. Mutually exclusive with content."
}
```

```rust
"radius": {
    "description": "Number of lines above and below target for semanticContext/securityContext source excerpt. semanticContext default 40/max 120; securityContext default 80/max 200."
}
```

For `include_call_hierarchy`, clarify both contexts:

```rust
"description": "Include call hierarchy section in semanticContext. In securityContext, call hierarchy defaults to true when line+column are supplied. Requires line+column."
```

Acceptance criteria:

- schema snapshot is updated;
- descriptions accurately reflect `securityContext` behavior;
- no misleading semanticContext-only language remains for shared fields.

## Phase 4 — Expand `architecture/lsp.md`

Add a dedicated section after semantic context or hierarchy docs:

```markdown
### Security context packets

`securityContext` is a read-only context-gathering operation for security review. It is not a vulnerability scanner and does not produce vulnerability verdicts.

It combines:
- bounded source excerpt;
- deterministic risk markers;
- security-relevant diagnostics and symbols;
- definitions and references when a target position is supplied;
- shallow call hierarchy when a target position is supplied;
- optional overlay diagnostics for proposed full content or a single-file patch.

It never writes proposed content to disk. Patch/content input is applied only in memory through the existing semantic overlay path.
```

Document supported risk categories:

```text
auth, crypto, filesystem, network, process, unsafe, serialization, sql, secrets, path_traversal, concurrency
```

Document limits:

```text
risk markers: default 80, max 200
excerpt radius: default 80, max 200
security diagnostics: max 80
security symbols: max 80
references: max 80
```

Add a note:

```markdown
Risk markers are deterministic keyword/identifier/context matches with rationale strings. They are prompts for review, not evidence of a confirmed vulnerability.
```

Acceptance criteria:

- docs clearly distinguish context retrieval from vulnerability detection;
- docs list categories and limits;
- docs state no mutation/no command execution;
- docs mention overlay behavior is in-memory only.

## Phase 5 — Expand Hierarchy Docs

Add or complete a hierarchy section in `architecture/lsp.md`:

```markdown
### Hierarchy operations

`callHierarchy` and `typeHierarchy` are read-only code-intelligence operations. They require `file_path`, `line`, and `column`. Both default to `direction="both"`.

`callHierarchy` maps:
- `incoming` → callers of the target symbol
- `outgoing` → calls made by the target symbol

`typeHierarchy` maps:
- `incoming` → supertypes
- `outgoing` → subtypes

The first implementation is shallow and non-recursive. It prepares the target hierarchy item and requests immediate relationships only. Unsupported language servers may return empty sections or per-section error fields.

`semanticContext` can include hierarchy sections with `include_call_hierarchy=true` or `include_type_hierarchy=true`. These flags require `line` and `column`; requests without a target position are rejected.

`securityContext` includes call hierarchy by default when a target position is supplied unless disabled by input behavior in future presets.
```

Acceptance criteria:

- hierarchy behavior is documented beyond the key-responsibility bullet;
- type-hierarchy incoming/outgoing mapping is explicit;
- unsupported-server behavior is documented;
- semanticContext hierarchy position requirement is documented.

## Phase 6 — Update Tool/Skill Docs

Update where present:

```text
architecture/tool.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
```

Add concise operation notes:

```markdown
- `securityContext`: read-only security-review packet. Returns deterministic risk markers plus bounded LSP context. Not a vulnerability scanner. Never writes files.
- `callHierarchy` / `typeHierarchy`: read-only, shallow, bounded hierarchy summaries. Require `file_path`, `line`, and `column`.
```

For `.opencode/skills/lsp/SKILL.md`, include usage guidance:

```markdown
Use `securityContext` before a security review of a target symbol or proposed patch. Treat risk markers as review prompts, not findings. Use explicit mutating tools only after reviewing returned patches or context.
```

Acceptance criteria:

- tool docs expose securityContext purpose and limitations;
- skill docs guide agent usage safely;
- no docs suggest `securityContext` confirms vulnerabilities.

## Phase 7 — Tests

Add/update tests:

```text
securityContext_top_level_truncated_false_when_limits_clear
securityContext_top_level_truncated_true_when_marker_limit_truncated
structured_lsp_provenance_reflects_truncated_field
lsp_schema_descriptions_include_securityContext_overlay
```

If top-level truncation is easiest to test through temp content, generate more than `max_risk_markers` risk lines:

```json
{
  "operation": "securityContext",
  "file_path": "...",
  "max_risk_markers": 3
}
```

Assert:

```rust
assert_eq!(parsed["truncated"], true);
assert_eq!(parsed["results"]["limits"]["risk_markers_truncated"], true);
```

For non-truncated exact-cap:

```rust
max_risk_markers = number_of_markers
```

Assert both false.

Acceptance criteria:

- top-level truncation behavior is covered;
- structured provenance truncation is covered if test harness can call structured execution;
- schema description snapshot is updated.

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
cargo test --test lsp securityContext
cargo test --test lsp security_context
cargo test -p codegg lsp_parameters_schema_snapshot
cargo test -p codegg structured_lsp_provenance_reflects_truncated_field
rg "securityContext" architecture/lsp.md architecture/tool.md .opencode/skills/lsp/SKILL.md AGENTS.md src/tool/lsp.rs tests/lsp.rs
rg "not a vulnerability scanner|risk markers|vulnerability verdict|read-only context" architecture/lsp.md architecture/tool.md .opencode/skills/lsp/SKILL.md AGENTS.md
rg "truncated: false" src/tool/lsp.rs
```

Review any remaining `truncated: false` manually. Some operations may legitimately never truncate, but `securityContext` should not be one of them.

## Done Criteria

This pass is complete when:

- `securityContext` top-level `truncated` reflects nested security limit flags;
- structured tool provenance reflects top-level truncation;
- shared schema descriptions mention `securityContext` where relevant;
- docs clearly explain `securityContext` as bounded context, not vulnerability detection;
- docs fully describe hierarchy operation behavior;
- tests cover top-level truncation and schema wording;
- no mutation, command execution, or scanner behavior change is introduced.

## Next Pass After This

Move to configurable security presets:

```text
security_preset = rust_server | rust_cli | web_backend | dependency_review | unsafe_review
```

Presets should tune marker categories, default radius, hierarchy inclusion, and symbol/diagnostic prioritization without changing the read-only/no-mutation contract.
