# Phase 4: Model-Facing Eggsact Tool Subset

## Goal

Expose a conservative, Codegg-native subset of eggsact deterministic tools to the model. The default palette should improve correctness without overwhelming the prompt or encouraging the model to use tiny utility calls for every operation.

Phase 3 establishes the adapter. This phase turns selected eggsact tools into real Codegg `Tool` implementations and registers them according to profile, audience, and deferral policy.

## Default exposure principles

- Expose only tools that materially reduce common coding-agent failure modes.
- Prefer Codegg-native names and descriptions over raw eggsact names if a wrapper can clarify use.
- Keep harness-only eggsact tools out of model definitions.
- Mark contextual/expert tools as deferred when available through `tool_search`.
- Avoid duplicating existing Codegg tools unless the eggsact-backed version has clear correctness value.

## Recommended initial model-facing set

### Always visible or near-core

- `text_equal`
- `text_diff_explain`
- `text_replace_check`
- `validate_json`
- `validate_toml`
- `regex_safety_check`
- `command_preflight`
- `path_normalize`
- `text_security_inspect`

### Deferred/contextual

- `text_inspect`
- `line_range_extract`
- `line_range_compare`
- `config_preflight`
- `identifier_inspect`
- `structured_data_compare`
- `path_compare`
- `argv_compare`
- `shell_quote_join`

### Do not expose by default

- Full math/unit tools unless Codegg wants a calculator surface.
- Heavy repo audit tools until active context policy is in place.
- Expert-only or debug-only eggsact tools.
- Harness-only tools as model-visible definitions.

## Implementation steps

### 1. Add a generic `EggsactTool` wrapper

Create a reusable wrapper around the adapter from Phase 3.

Suggested shape:

```rust
pub struct EggsactTool {
    codegg_name: &'static str,
    eggsact_name: &'static str,
    description: &'static str,
    parameters: fn() -> serde_json::Value,
    category: ToolCategory,
    defer_loading: bool,
}
```

The wrapper should implement:

- `Tool::name`
- `Tool::description`
- `Tool::parameters`
- `Tool::category`
- `Tool::defer_loading`
- `Tool::execute`
- `Tool::execute_structured`

Use eggsact's own schema views if available. If not, define compact Codegg-facing schemas and translate to eggsact input names.

### 2. Decide naming strategy

Use raw eggsact names only when they are already clear and stable, such as `text_equal`, `validate_json`, and `validate_toml`.

Use Codegg-prefixed or workflow-oriented names when the raw name is ambiguous. For example:

- `patch_replace_check` could wrap eggsact `text_replace_check` if Codegg wants a more workflow-specific name.
- `shell_command_preflight` could wrap eggsact `command_preflight` if the current `bash` flow needs clearer disambiguation.

Avoid exposing both a raw and renamed wrapper for the same operation.

### 3. Register model-safe wrappers

Add registration to `ToolRegistry::with_options` after core file/search tools and before `tool_search` catalog finalization.

Registration should consult config:

- Disabled deterministic tools should not register.
- Model-facing profile should default to `codegg_core_min` or a similarly narrow Codegg profile.
- Expert/contextual tools should be deferred unless explicitly configured visible.

### 4. Preserve permission semantics

Most eggsact model tools should be `ToolCategory::ReadOnly` because they operate on model-provided strings. If a wrapper later reads files, walks paths, or inspects local project state, do not classify it as blindly read-only without checking Codegg's permission model.

`command_preflight` is read-only as a validator, not a shell executor. Its description must make that explicit so the model does not confuse it with `bash`.

### 5. Output format

Return deterministic, compact output.

Recommended envelope:

```text
[deterministic_tool source=eggsact tool=text_replace_check trust=local_trusted]
ok: true
machine_code: ok
result:
...
findings:
...
[/deterministic_tool]
```

For high-volume findings, apply Codegg-side output caps even if eggsact already applies budgets.

### 6. Tool descriptions

Descriptions should state when to use the tool and when not to use it.

Example for `text_replace_check`:

```text
Deterministically check whether a proposed textual replacement is exact and unambiguous before editing. Use before replace/edit operations when whitespace, duplicate matches, or Unicode confusables could matter. Does not modify files.
```

Example for `command_preflight`:

```text
Analyze a shell command for quoting, path, regex, and safety issues before execution. This does not run the command.
```

### 7. Deferred discovery

Ensure deferred eggsact tools appear in the catalog and can be found through `tool_search`. If the existing catalog only indexes registered visible tools, update registration so deferred tools can be discovered without sending full schemas in every request.

### 8. Tests

Add tests for:

- Default model definitions include only the intended always-visible eggsact tools.
- Deferred eggsact tools are omitted from default definitions but discoverable.
- Harness-only tools are not model-visible.
- Tool descriptions do not claim mutation or execution where none occurs.
- Each wrapper calls the expected eggsact tool name.
- Each wrapper returns local trusted provenance.
- Unknown/invalid input is surfaced as deterministic failure, not panic.

## Acceptance criteria

- A narrow eggsact-backed model-facing subset is registered.
- Contextual eggsact tools are deferred/discoverable rather than always injected.
- Harness-only eggsact tools are not exposed to the model.
- Tool output is bounded and framed as deterministic local utility output.
- Provenance and tool categories are correct.
- Existing Codegg tool behavior is not regressed.

## Risks

The risk is prompt bloat and utility-call overuse. Keep the default palette strict. More tools can be enabled by config after telemetry and qualitative testing show they improve outcomes.
