# Security Review Executor Injection Follow-Up Plan

## Purpose

Complete the remaining gap in the real LSP security review enrichment path: the concrete `LspSecurityContextExecutor` adapter exists, but `/security-review --enrich` still needs a clean runtime injection path so it can use a real executor when available and clearly fall back when it is not.

Current state:

- `src/security/lsp_executor.rs` provides `LspSecurityContextExecutor` wrapping `LspTool`.
- `validate_security_context_request()` exists and rejects invalid or mutation-shaped requests.
- `SecurityContextExecutor`, `NoopSecurityContextExecutor`, and `FixtureSecurityContextExecutor` exist.
- `run_security_review_workflow_with_lsp_enrichment()` exists.
- `/security-review --enrich` parses successfully.
- The TUI currently passes no real executor pending LSP state accessibility.

This pass should avoid broad architectural churn. Focus on injection plumbing and explicit runtime behavior.

## Non-Goals

Do not rewrite the LSP tool implementation.

Do not make enrichment default.

Do not remove `NoopSecurityContextExecutor` or `FixtureSecurityContextExecutor`.

Do not require a live LSP server for unit tests.

Do not mutate files.

Do not add dependency/CVE lookup.

Do not add network scanning.

Do not generate exploit payloads or offensive guidance.

## Phase 1 — Add Command Runner With Optional Executor

Add a command runner that accepts an optional executor instead of hardwiring no-op behavior.

Recommended signature:

```rust
pub async fn run_security_review_command_with_executor(
    root: &Path,
    args: &SecurityReviewCommandArgs,
    executor: Option<&dyn SecurityContextExecutor>,
) -> Result<String, String>
```

Behavior:

1. Parse command args into `SecurityReviewWorkflowOptions` exactly as the current command runner does.
2. If `args.enrich == false`, call `run_security_review_workflow` and do not touch the executor.
3. If `args.enrich == true` and `executor.is_some()`, call `run_security_review_workflow_with_lsp_enrichment` with that executor.
4. If `args.enrich == true` and `executor.is_none()`, return deterministic output plus a clear note:

```text
LSP enrichment requested but no securityContext executor is available in this runtime.
```

5. Preserve JSON/text rendering behavior.

Keep the existing:

```rust
pub async fn run_security_review_command(
    root: &Path,
    args: &SecurityReviewCommandArgs,
) -> Result<String, String>
```

as a compatibility wrapper:

```rust
run_security_review_command_with_executor(root, args, None).await
```

Acceptance criteria:

- deterministic default path is unchanged;
- `--enrich` with `None` produces deterministic output plus explicit note;
- `--enrich` with `Some(executor)` uses the injected executor;
- existing tests continue to compile.

## Phase 2 — Add Provider Trait or Equivalent Runtime Hook

Add a small provider abstraction only if it fits the current runtime shape.

Preferred trait:

```rust
pub trait SecurityContextExecutorProvider {
    fn security_context_executor(&self) -> Option<Arc<dyn SecurityContextExecutor>>;
}
```

Alternative: skip the trait and pass `Option<Arc<dyn SecurityContextExecutor>>` directly through the TUI command handler if that is simpler.

Rules:

- keep this at the boundary layer, not inside evidence/synthesis code;
- do not force every runtime to create an LSP executor;
- do not require TUI state inside `src/security/workflow/*`;
- provider may return `None` when no LSP state is available.

Acceptance criteria:

- there is a clear hook for runtimes to provide `LspSecurityContextExecutor`;
- the hook is optional;
- no-LSP runtimes remain valid.

## Phase 3 — Locate TUI/Core LSP Availability

Inspect the TUI/core command path and `LspTool` ownership.

Search:

```bash
rg "LspTool" src crates -g '*.rs'
rg "COMMAND_REGISTRY|security-review|run_security_review_command" src/tui src -g '*.rs'
rg "CoreClient|CoreRequest|Tool|tool" src/tui src/tool src/protocol -g '*.rs'
rg "securityContext" src/tool src crates -g '*.rs'
```

Determine whether `/security-review` currently runs in a context that can construct or access:

```rust
Arc<LspTool>
```

or an equivalent LSP/tool operation handle.

Possible outcomes:

### Outcome A — TUI has direct or safe access to `Arc<LspTool>`

Create `LspSecurityContextExecutor::new(arc_lsp_tool)` and pass it to `run_security_review_command_with_executor` for `--enrich`.

### Outcome B — TUI only has a core/client abstraction

Add a client-backed executor adapter in the boundary layer if the client can issue the LSP tool operation cleanly. Keep it JSON-in/JSON-out and implement `SecurityContextExecutor`.

### Outcome C — No clean runtime access yet

Leave TUI passing `None`, but make this explicit in code comments and output notes. Do not fake successful enrichment.

Acceptance criteria:

- chosen outcome is documented in code or docs;
- no unsafe dependency inversion is introduced;
- no duplicate LSP traversal logic is created.

## Phase 4 — Wire TUI Command Handler

Update the TUI command handler for `/security-review` to call the new executor-aware runner.

Current target behavior:

```rust
let args = parse_security_review_args(input);
let executor = app_or_runtime.security_context_executor();
let rendered = run_security_review_command_with_executor(root, &args, executor.as_deref()).await?;
```

If no executor exists:

```rust
let rendered = run_security_review_command_with_executor(root, &args, None).await?;
```

Important:

- do not instantiate `NoopSecurityContextExecutor` in the user-facing command path unless it is only a local wrapper for `None` semantics;
- prefer `None` and explicit note;
- ensure `--json --enrich` includes the unavailable/executed note in output JSON.

Acceptance criteria:

- `/security-review --changed` remains deterministic/no-LSP;
- `/security-review --changed --enrich` uses real executor when available;
- `/security-review --changed --enrich` without executor returns deterministic output plus note;
- text and JSON output both expose enrichment status.

## Phase 5 — Improve Enrichment Status Notes

Make notes precise in all enrichment states.

Add helpers if needed:

```rust
fn note_lsp_enrichment_unavailable(output: &mut SecurityReviewOutput)
fn note_lsp_enrichment_no_eligible_targets(output: &mut SecurityReviewOutput)
fn note_lsp_enrichment_executed(output: &mut SecurityReviewOutput, count: usize)
```

Expected notes:

```text
LSP enrichment requested but no securityContext executor is available in this runtime.
LSP enrichment requested but no targets met escalation policy.
LSP enrichment executed N request(s).
```

Avoid ambiguous notes such as only `executor error: no securityContext executor available` for the no-executor runtime. That is correct at the internal executor level but weak for user-facing output.

Acceptance criteria:

- unavailable, no-eligible-targets, executed, timeout, and executor-error cases are distinguishable;
- JSON mode carries the same notes;
- tests assert notes.

## Phase 6 — Tests

Add tests without a live LSP server.

### Command runner tests

```text
security_review_command_with_executor_default_does_not_call_executor
security_review_command_with_executor_enrich_uses_fixture_executor
security_review_command_with_executor_enrich_none_notes_unavailable
security_review_command_with_executor_json_includes_enrichment_note
security_review_command_with_executor_prompts_only_still_respects_enrich
security_review_command_with_executor_findings_only_still_respects_enrich
```

### Provider/wiring tests

If a provider trait is added:

```text
security_context_executor_provider_none_is_allowed
security_context_executor_provider_some_is_used
```

### TUI command handler tests

If existing TUI command tests exist, add:

```text
tui_security_review_enrich_passes_executor_when_available
tui_security_review_enrich_without_executor_fails_soft
```

If TUI tests are too heavy, test the extracted handler function instead.

### Request validation regression

Keep existing `validate_security_context_request` tests. Add one regression if command injection passes request through real adapter:

```text
security_review_enrich_injected_executor_rejects_mutation_request
```

Acceptance criteria:

- no test requires a live language server;
- fixture executor request count proves whether enrichment ran;
- no-executor output has explicit unavailable note;
- default command path does not touch executor.

## Phase 7 — Docs Updates

Update:

```text
README.md
AGENTS.md
architecture/lsp.md
architecture/tool.md
.opencode/skills/security/SKILL.md
```

Document actual runtime behavior after this pass.

If real executor is wired:

```text
/security-review --enrich uses the real LSP securityContext executor when the runtime has LSP state available. Otherwise it returns deterministic output with an unavailable note.
```

If real executor is still not reachable from TUI:

```text
The real LspSecurityContextExecutor exists for runtimes that can provide Arc<LspTool>, but the current TUI command path still runs with no executor and reports that enrichment is unavailable.
```

Acceptance criteria:

- docs match actual wiring;
- docs do not imply enrichment is always available;
- docs preserve read-only/no-mutation/no-exploit semantics.

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
cargo test -p codegg security_review_command_with_executor
cargo test -p codegg security_context_executor_provider
cargo test -p codegg security_review_enrich
cargo test -p codegg security_context_request_validation
rg "run_security_review_command_with_executor|SecurityContextExecutorProvider|LspSecurityContextExecutor|NoopSecurityContextExecutor" src crates tests
rg "LSP enrichment requested but no securityContext executor" src tests README.md AGENTS.md architecture .opencode
rg "--enrich|security-review" src/tui src/security README.md AGENTS.md architecture .opencode
```

Manual smoke:

```text
1. Run /security-review --changed. Confirm no enrichment note and no LSP call.
2. Run /security-review --changed --enrich in a runtime without executor. Confirm deterministic output plus unavailable note.
3. Run /security-review --changed --json --enrich without executor. Confirm JSON notes contain unavailable note.
4. In a test harness with FixtureSecurityContextExecutor, confirm --enrich invokes executor and merges returned prompts/evidence.
5. If TUI can access real LspTool, run --enrich in a Rust repo with LSP active and confirm bounded request count note.
```

## Done Criteria

This pass is complete when:

- `run_security_review_command_with_executor` exists;
- existing `run_security_review_command` delegates to the executor-aware runner with `None`;
- `/security-review --enrich` can use an injected real executor where available;
- no-executor runtimes return deterministic output plus explicit unavailable note;
- default `/security-review` behavior remains unchanged;
- tests prove executor use, no-executor fallback, and default no-LSP behavior;
- docs accurately state current executor availability;
- no mutation, network scanning, shell execution, exploit generation, or unbounded call expansion is introduced.

## Follow-Up Passes

After this lands, likely next targets are:

1. Interactive TUI findings panel with enrichment status and navigation.
2. Project-level security policy config for thresholds, budgets, and ignored paths.
3. Dependency/CVE enrichment for dependency review targets.
4. Security reviewer agent prompt integration using enriched review receipts.
