# LSP Phase 14 Plan: Workflow Composition UX

Status date: 2026-06-26
Phase type: TUI / agent workflow UX / recipe composition
Prerequisites: Phase 13 validation and `/lsp-doctor` substantially complete.

## Purpose

Phase 14 should expose the existing LSP workflow recipes as user- and agent-facing workflows. Phases 7 and 10 created the recipe layer. The next bottleneck is usability: users and smaller models should not need to know which low-level LSP evidence operations to call, which caps apply, or how to interpret stale/truncated packets.

The phase goal is to make common repair, review, security, impact, test-failure, interface-boundary, cross-file, and call-neighborhood workflows invokable by intent.

## Current baseline

The repo already has named recipes such as:

- `repair_local`,
- `repair_hunk`,
- `review_file`,
- `review_diff`,
- `security_review_enriched`,
- `hunk_source_navigation`,
- `preview_suggestion`,
- `impact_analysis`,
- `test_failure_repair`,
- `interface_boundary`,
- `cross_file_repair`,
- `call_neighborhood`.

The repo also has `RecipeSettings`, `RecipeOutcome`, model-tier defaults, renderer support, preview IDs, stale notes, and policy summaries.

## Non-goals

Do not add new semantic operations.

Do not expose raw arbitrary LSP protocol requests through TUI commands.

Do not make workflows apply previews automatically.

Do not execute server commands.

Do not make recipe commands mutate files.

Do not add long-running autonomous agents in this phase.

## Workstream 1: define user-facing workflow command surface

### Proposed commands

Use concise names that map to existing recipes:

- `/lsp-repair-local <path[:line]>`
- `/lsp-repair-hunk <path> [hunk-id|range]`
- `/lsp-review-file <path>`
- `/lsp-review-diff`
- `/lsp-security-review [path|diff]`
- `/lsp-impact <path:line:col>`
- `/lsp-test-repair <test-file> [failure-text]`
- `/lsp-interface <path[:symbol]>`
- `/lsp-cross-repair <primary> [related...]`
- `/lsp-call-neighborhood <path:line:col> [incoming|outgoing|both]`

If too many commands would clutter the slash-command list, use one command with subcommands:

```text
/lsp-workflow repair-local ...
/lsp-workflow impact ...
/lsp-workflow test-repair ...
```

Prefer the shape that fits existing command parser conventions.

### Target files

- `src/tui/command.rs`
- `src/tui/app/mod.rs`
- `src/tool/lsp.rs`
- `crates/egglsp/src/workflow_recipes.rs`
- `crates/egglsp/src/tui_summary.rs`
- command dispatch tests

### Acceptance criteria

- Every surfaced recipe has a clear command or subcommand.
- Missing-arg and invalid-arg cases produce usage text.
- Commands are read-only and never apply previews.

## Workstream 2: recipe invocation boundary

### Problem

Recipe execution likely needs an adapter that turns TUI command args into `LspWorkflowRecipe`, `RecipeSettings`, and request inputs without duplicating orchestration.

### Proposed design

Add a narrow execution function in `LspTool` or `egglsp`:

```rust
pub async fn run_lsp_workflow(
    &self,
    request: LspWorkflowInvocation,
) -> Result<LspWorkflowDisplay, ToolError>
```

Where `LspWorkflowInvocation` includes:

- recipe kind,
- primary path,
- optional position,
- optional hunk/range,
- optional related files,
- optional failure text,
- optional model tier,
- review/security mode.

The function should return rendered evidence plus structured metadata:

- stale evidence count,
- truncation count,
- preview IDs,
- unsupported operation notes,
- policy summary,
- suggested next action.

### Implementation steps

1. Reuse existing recipe functions rather than duplicating collector calls.
2. Resolve paths through existing allowed-root validation.
3. Resolve model tier using existing policy defaults.
4. Return compact display text suitable for TUI toast/panel.
5. Preserve structured metadata for future panels.

### Acceptance criteria

- Workflow command implementation is thin over recipe APIs.
- Recipe output preserves stale/truncation/preview metadata.
- The boundary is testable without a live server via mock provider or fake service.

## Workstream 3: TUI display model

### Problem

Workflow results may be too large for a toast. They need predictable presentation.

### Display options

Choose the smallest viable UI:

- Toast for short summary plus preview IDs.
- Detail panel for full rendered recipe output.
- Scrollable modal if existing infrastructure supports it.
- Copy/export path only if already available.

### Required sections

Every workflow display should include:

- title and recipe name,
- target path/symbol/test,
- evidence count,
- stale/fresh summary,
- truncation summary,
- unsupported operation notes,
- preview IDs, if any,
- suggested next command, such as `/lsp-preview <id>` or `/lsp-doctor <path>`.

### Acceptance criteria

- Long recipe output does not overflow an unreadable toast.
- Users can inspect preview IDs and stale notes.
- Output is consistent across workflows.

## Workstream 4: agent-facing workflow intent mapping

### Purpose

Agents, especially smaller models, should be able to ask for high-level workflow help without manually composing low-level tool arguments.

### Target files

- `src/tool/lsp.rs`
- tool schema definitions
- `.opencode/skills/lsp/SKILL.md`
- `architecture/lsp.md`

### Implementation steps

1. Add a small set of intent names matching recipes.
2. Provide compact examples in the LSP skill file.
3. Keep required fields minimal; infer mode from provided file/hunk/diff metadata when possible.
4. Include explicit freshness/truncation notes in output.
5. Avoid agent-facing raw protocol names unless useful for debugging.

### Acceptance criteria

- Smaller models can invoke workflow-level operations with few fields.
- Tool output suggests next actions without applying changes.

## Workstream 5: workflow composition rules

### Purpose

Some workflows should compose multiple recipes. Keep composition bounded and explicit.

### Candidate compositions

- `security_review` = deterministic security review + `security_review_enriched` + optional call-neighborhood for high-risk findings.
- `repair_failing_test` = `test_failure_repair` + `repair_local` for likely source file + preview suggestions, if safe.
- `review_api_change` = `interface_boundary` + `impact_analysis`.
- `repair_hunk` = `repair_hunk` + preview suggestion only if changed lines are fresh.

### Rules

- No composition may scan the whole workspace.
- Each composition must have an explicit cap.
- Each composition must record which sub-recipes ran and which were skipped.
- Stale evidence must be visible.
- Preview suggestions remain preview-only.

### Acceptance criteria

- At least two high-value composed workflows are implemented or explicitly deferred.
- Composition output shows sub-recipe provenance and caps.

## Workstream 6: tests

### Required tests

- command parser/dispatch tests for each workflow command or subcommand,
- missing args and invalid paths,
- no-tool/LSP unavailable behavior,
- recipe invocation with mock/fake provider,
- output includes stale and truncation notes,
- preview IDs are displayed but not applied,
- workflow composition caps and skip reasons,
- agent-facing schema examples, if schemas are tested.

### Focused commands

```bash
cargo fmt --check
cargo test -p egglsp workflow_recipes
cargo test -p egglsp context_renderer
cargo test --test phase5_context_integration lsp
```

Broader:

```bash
cargo test -p egglsp
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

## Documentation

Update:

- `architecture/lsp.md`,
- `.opencode/skills/lsp/SKILL.md`,
- `README.md` command list if public,
- `AGENTS.md` verified facts.

Document:

- workflow command list,
- examples,
- input syntax,
- output sections,
- safety invariants,
- preview-only behavior,
- stale/truncation behavior.

## Final acceptance criteria

Phase 14 is complete when:

- common LSP recipes are invokable from TUI or a consolidated workflow command,
- agent-facing intent mapping exists for the same workflows,
- workflow output is readable and consistent,
- commands are read-only and never auto-apply previews,
- tests cover dispatch, invalid args, stale/truncation display, preview IDs, and at least two composed workflows or explicit deferrals.
