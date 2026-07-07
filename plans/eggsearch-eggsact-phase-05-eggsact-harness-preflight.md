# Phase 5: Harness-Side Eggsact Preflight Integration

## Goal

Use eggsact internally as Codegg's deterministic preflight substrate before operations that commonly fail due to exact text mismatch, path ambiguity, malformed config, shell quoting, regex hazards, Unicode confusables, or unsafe command structure.

This phase is primarily harness-side. The model does not need to explicitly call these tools for Codegg to benefit.

## Scope

Wire eggsact preflights into:

- Patch application.
- Replace/edit workflows.
- File writes that target structured config.
- Shell command execution approval/dispatch.
- Sensitive path checks.
- Unicode and identifier security checks where relevant.
- Security-review evidence preparation where deterministic checks reduce noise.

## Execution policy

Preflights should be classified by severity:

- `block`: deterministic violation that would make the operation incorrect or unsafe.
- `warn`: likely issue that should be surfaced to the model or user but may not block.
- `annotate`: informational finding added to logs/provenance only.

Default behavior should be conservative but not obstructive. For example:

- Exact replacement not found: block.
- Replacement has multiple candidate matches when the operation expects one: block or require disambiguation.
- TOML/JSON invalid after config write: block if the target file was valid before or is known config.
- Command has shell syntax hazard or dangerous construct: warn or escalate to existing permission flow.
- Unicode confusable in code identifier: warn unless the security policy says block.

## Implementation steps

### 1. Add `preflight` service module

Create a module such as `src/preflight/eggsact.rs` or `src/tool/preflight.rs` that owns harness-side calls.

Suggested API:

```rust
pub struct PreflightService {
    eggsact: EggsactRuntime,
    policy: PreflightPolicy,
}

pub struct PreflightFinding {
    pub severity: PreflightSeverity,
    pub machine_code: Option<String>,
    pub message: String,
    pub location: Option<PreflightLocation>,
    pub source_tool: String,
}

pub enum PreflightDecision {
    Allow { findings: Vec<PreflightFinding> },
    Warn { findings: Vec<PreflightFinding> },
    Block { findings: Vec<PreflightFinding> },
}
```

Keep this layer separate from model-facing eggsact wrappers so internal checks can use `ToolAudience::Harness`.

### 2. Patch and replace preflight

Before applying replacements or patches, run relevant checks:

- `text_replace_check` for exact replacement uniqueness.
- `line_range_extract` for line-bounded edits.
- `text_diff_explain` for diagnostics when a patch fails.
- `path_scope_check` or `path_batch_scope_check` for target path policy.

Integration targets:

- `src/tool/apply_patch.rs`
- `src/tool/replace.rs`
- `src/tool/edit.rs`
- `src/tool/multiedit.rs`

Do not duplicate the same expensive check at every layer. Prefer one preflight service call in the highest-level operation that has all necessary context.

### 3. Config write preflight

For writes or edits targeting known config files, run format validation after generating the candidate new content but before committing it to disk when possible.

Use:

- `validate_json`
- `validate_toml`
- `config_preflight`
- `structured_data_compare` when comparing before/after shape is useful.

Candidate file patterns:

- `*.json`
- `*.toml`
- `.env`
- `Cargo.toml`
- `package.json`
- config files already known by Codegg's config parser.

If pre-write validation is not practical for an operation, perform post-write validation and surface a warning plus rollback guidance where possible.

### 4. Shell command preflight

Before `bash` or human shell commands execute through model-controlled paths, call eggsact `command_preflight` and possibly `regex_safety_check` for commands containing regex arguments.

Integrate findings into existing permission and destructive-command logic. Do not replace current permission checks. Instead, enrich them:

- Dangerous command pattern found by Codegg and eggsact: escalate.
- Eggsact syntax/quoting warning: show in prompt/log.
- Regex backtracking hazard: warn or block based on policy.
- Command preflight failure due to malformed shell syntax: block unless user explicitly overrides.

### 5. Unicode and identifier safety

Use eggsact tools such as `text_security_inspect`, `text_inspect`, and `identifier_inspect` where relevant:

- Newly added identifiers in patches.
- Suspicious prompt-supplied file paths.
- Security review workflows.
- Tool-generated code snippets if Codegg can cheaply isolate changed text.

Default action should be warn, not block, unless a policy flag explicitly enables blocking.

### 6. Preflight policy config

Add config such as:

```toml
[preflight]
enabled = true
mode = "warn"            # "off" | "observe" | "warn" | "block_on_definite"
patch = true
config = true
shell = true
unicode = true
path_scope = true
log_findings = true
model_visible_findings = true
```

The initial rollout should support `observe` and `warn` well. `block_on_definite` should be limited to deterministic correctness failures.

### 7. Surface findings

Findings should appear in the right place:

- For model tool calls: tool output should include concise preflight findings.
- For logs: structured tracing fields should include tool, machine_code, severity, and decision.
- For TUI: show warnings near the operation result or permission prompt.
- For tests: findings should be inspectable without string scraping where practical.

### 8. Avoid recursive tool pollution

Harness preflights should not appear as separate model tool calls in the conversation unless explicitly useful. They are internal validation events. Do not inflate transcript history with every internal eggsact call.

If preflight findings need model-visible feedback, summarize them in the parent tool result.

## Validation

Add tests for:

- Replacement not found blocks before file mutation.
- Ambiguous replacement blocks or warns according to policy.
- Valid JSON/TOML config write passes.
- Invalid JSON/TOML config write warns or blocks according to policy.
- Shell command with suspicious quoting surfaces a preflight finding.
- Regex safety finding is included for regex-heavy commands.
- Unicode confusable finding is warn-only by default.
- Observe mode logs but does not alter behavior.
- Harness preflights do not appear in model tool definitions.

## Acceptance criteria

- Eggsact preflights run automatically in key edit/config/shell paths.
- Findings are structured and severity-classified.
- Definite correctness failures can block mutation.
- Warnings enrich existing permission and tool output paths.
- Internal preflight calls do not bloat the model-facing tool palette or transcript.

## Risks

The main risk is accidental behavior regression in core edit and shell flows. Roll out behind config with `observe` or `warn` defaults, and only block deterministic errors with very high confidence.
