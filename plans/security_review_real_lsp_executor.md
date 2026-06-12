# Security Review Real LSP Executor Integration Plan

## Purpose

Wire the optional `SecurityContextExecutor` enrichment path to the real Codegg LSP/securityContext operation boundary so `/security-review --enrich` can execute actual bounded, read-only LSP enrichment when the runtime has an LSP-capable executor available.

Current state:

- `SecurityContextExecutor` trait exists.
- `NoopSecurityContextExecutor` and `FixtureSecurityContextExecutor` exist.
- `run_security_context_enrichment()` is bounded and fail-soft.
- `run_security_review_workflow_with_lsp_enrichment()` performs the two-stage review.
- `/security-review --enrich` parses and routes to enriched workflow.
- The command currently uses `NoopSecurityContextExecutor`, so user-facing enrichment is safe but not real.

This pass should add the real adapter without coupling security workflow modules directly to TUI state or mutating source files.

## Non-Goals

Do not add dependency/CVE lookup.

Do not add network scanning.

Do not mutate files.

Do not generate exploit payloads or offensive guidance.

Do not make enrichment default.

Do not remove `NoopSecurityContextExecutor` or `FixtureSecurityContextExecutor`.

Do not require a live language server for unit tests.

Do not call the LSP tool from evidence synthesis directly.

## Phase 1 — Locate the Existing LSP Operation Boundary

Inspect the existing LSP/securityContext call path before writing integration code.

Search:

```bash
rg "securityContext" src crates -g '*.rs'
rg "build_security_context|security_context|SecurityContext" src crates -g '*.rs'
rg "CoreRequest|CoreResponse|ToolRequest|tool_call|Lsp" src crates -g '*.rs'
rg "COMMAND_REGISTRY|run_security_review_command|TuiCommand|tui_cmd" src/tui src -g '*.rs'
```

Identify:

1. the lowest-level function that executes the `securityContext` LSP operation;
2. whether it requires direct access to LSP manager state, tool runtime state, or core client state;
3. whether TUI commands currently run in a context that can access that state;
4. how errors/timeouts are represented.

Deliverable:

- a short inline comment or doc note in the new adapter explaining why the selected boundary was chosen.

Acceptance criteria:

- integration target is explicit;
- no duplicate LSP implementation is created;
- no workflow module imports TUI UI state.

## Phase 2 — Add a Real Executor Adapter

Add a runtime adapter implementing `SecurityContextExecutor`.

Preferred shape if there is a reusable LSP operation/service:

```rust
pub struct LspSecurityContextExecutor {
    // minimal handle to existing LSP/tool operation boundary
}

#[async_trait::async_trait]
impl SecurityContextExecutor for LspSecurityContextExecutor {
    async fn security_context(
        &self,
        request: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        // delegate to existing securityContext operation
    }
}
```

If the LSP boundary lives in core/tool infrastructure, put the adapter near that boundary rather than in TUI UI code.

Candidate locations:

```text
src/security/workflow/context.rs       # only if it can depend on a lightweight trait/handle
src/security/lsp_executor.rs           # preferred if concrete adapter needs crate-local wiring
src/tool/lsp.rs                        # only if existing tool boundary owns the operation
src/tui/tui_cmd.rs                     # only as a thin caller/bridge, not executor logic
```

Rules:

- adapter must be read-only;
- adapter must call the existing `securityContext` operation;
- adapter must not create its own call graph traversal logic;
- adapter must return `Err(String)` on unsupported server/tool failures;
- adapter must not panic on malformed JSON;
- request caps supplied by `build_escalated_security_context_request` must be preserved.

Acceptance criteria:

- a concrete executor exists when runtime has LSP access;
- it implements `SecurityContextExecutor`;
- unit tests can use a mocked lower boundary rather than live LSP.

## Phase 3 — Introduce a Runtime Executor Provider

Avoid hardwiring `NoopSecurityContextExecutor` inside the command handler.

Add a small provider abstraction where command execution can ask for an executor:

```rust
pub trait SecurityContextExecutorProvider {
    type Executor: SecurityContextExecutor;

    fn security_context_executor(&self) -> Option<&Self::Executor>;
}
```

If associated types are awkward, use:

```rust
pub trait SecurityContextExecutorProvider {
    fn security_context_executor(&self) -> Option<Arc<dyn SecurityContextExecutor>>;
}
```

Then add a higher-level command runner:

```rust
pub async fn run_security_review_command_with_executor(
    root: &Path,
    args: &SecurityReviewCommandArgs,
    executor: Option<&dyn SecurityContextExecutor>,
) -> Result<String, String>
```

Keep existing function:

```rust
run_security_review_command(root, args)
```

as no-LSP fallback or wrapper using `None`.

Acceptance criteria:

- command handler no longer has to directly instantiate `NoopSecurityContextExecutor` for all enrichment paths;
- no-executor path remains safe and fail-soft;
- test fixtures can inject a `FixtureSecurityContextExecutor` through command-level tests.

## Phase 4 — Wire `/security-review --enrich` to Runtime Executor

Find where `/security-review` is dispatched from the TUI command registry/handler.

Replace current behavior:

```rust
let executor = NoopSecurityContextExecutor;
run_security_review_workflow_with_lsp_enrichment(..., &executor)
```

with:

```rust
if args.enrich {
    match runtime.security_context_executor() {
        Some(executor) => run_security_review_command_with_executor(root, &args, Some(executor)).await,
        None => run_security_review_command_with_executor(root, &args, None).await,
    }
} else {
    run_security_review_command_with_executor(root, &args, None).await
}
```

If the TUI runtime cannot access LSP state yet:

- keep no-op fallback;
- expose a clear TODO and note in output;
- do not fake successful enrichment.

Acceptance criteria:

- `--enrich` uses real executor where available;
- runtime without executor returns deterministic review plus note;
- default `/security-review` does not initialize or invoke LSP enrichment;
- command output clearly reports whether enrichment executed or was unavailable.

## Phase 5 — Add Enrichment Availability Notes

Improve user-facing notes so `--enrich` state is unambiguous.

Cases:

```text
--enrich with real executor, requests executed:
  LSP enrichment executed N request(s).

--enrich with no executor:
  LSP enrichment requested but no securityContext executor is available in this runtime.

--enrich with no eligible targets:
  LSP enrichment requested but no targets met escalation policy.

--enrich with executor errors/timeouts:
  Keep per-target failure notes from enrichment results.
```

Acceptance criteria:

- users can distinguish no eligible targets from no executor;
- users can distinguish executor failure from deterministic-only output;
- JSON output contains notes as well.

## Phase 6 — Harden Request/Response Mapping

Before wiring a real executor, ensure request/response schema expectations are stable.

Add validation helper:

```rust
pub fn validate_security_context_request(request: &serde_json::Value) -> Result<(), String>
```

Validate:

- `file_path` exists and is a string;
- `security_preset` exists and is a string;
- `call_depth` is 0, 1, or 2 if present;
- `max_call_nodes` is within allowed cap if present;
- no mutation/action fields are present.

Optional deny-list fields:

```text
apply
write
edit
patch
command
execute
shell
```

Do not overfit if current tool schema already validates strongly; a lightweight local guard is enough.

Acceptance criteria:

- executor adapter validates request before invoking LSP;
- invalid request returns `Err(String)`;
- tests cover bad request shapes.

## Phase 7 — Tests

Use mocks/fixtures. Do not require a live LSP server.

### Adapter/provider tests

```text
security_context_executor_adapter_validates_request
security_context_executor_adapter_rejects_bad_call_depth
security_context_executor_adapter_rejects_missing_file_path
security_context_executor_adapter_preserves_caps
security_context_executor_adapter_maps_tool_error_to_err
```

### Command runner tests

```text
security_review_command_enrich_uses_injected_executor
security_review_command_enrich_without_executor_notes_unavailable
security_review_command_default_does_not_request_executor
security_review_command_enrich_no_eligible_targets_notes_no_targets
security_review_command_enrich_executor_failure_keeps_stage1_output
```

### Integration-shape tests

```text
security_review_enrich_fixture_response_promotes_expected_prompt
security_review_enrich_fixture_call_graph_adds_callpath_evidence
security_review_enrich_fixture_diagnostic_adds_diagnostic_evidence
```

### Regression tests

```text
security_review_marker_only_still_not_finding_after_real_executor_wiring
security_review_different_file_enriched_evidence_does_not_promote
security_review_no_mutation_fields_in_enrichment_request
```

Acceptance criteria:

- no unit test depends on a live language server;
- no test mutates source files;
- command tests prove executor injection works.

## Phase 8 — Docs and Skill Updates

Update:

```text
AGENTS.md
README.md
architecture/lsp.md
architecture/tool.md
.opencode/skills/security/SKILL.md
```

Document:

- `/security-review --enrich` now uses real `securityContext` executor when available;
- enrichment remains optional and read-only;
- default `/security-review` remains deterministic/no-LSP;
- no-executor runtimes fail soft;
- enrichment request limits and timeout defaults;
- no source mutation, shell execution, network scanning, or exploit generation.

Example:

```text
/security-review --changed
/security-review --changed --enrich
/security-review --base main --enrich --max-enriched-targets 4 --lsp-timeout-ms 1500
/security-review --changed --json --enrich
```

Acceptance criteria:

- docs match actual runtime behavior;
- docs do not claim enrichment is always available;
- docs preserve prompts vs findings semantics.

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
cargo test -p codegg security_context_executor
cargo test -p codegg security_review_command_enrich
cargo test -p codegg security_review_enrich
cargo test -p codegg security_context_request_validation
rg "LspSecurityContextExecutor|SecurityContextExecutorProvider|run_security_review_command_with_executor|validate_security_context_request" src crates tests
rg "NoopSecurityContextExecutor" src crates tests
rg "--enrich|security-review" src crates README.md AGENTS.md architecture .opencode
```

Manual smoke:

```text
1. Run /security-review --changed. Confirm no enrichment and no LSP call.
2. Run /security-review --changed --enrich without LSP runtime. Confirm deterministic output plus unavailable note.
3. Run /security-review --changed --enrich with LSP runtime available. Confirm bounded enrichment executes and notes N requests.
4. Run /security-review --changed --json --enrich. Confirm enrichment notes/evidence serialize.
5. Test unsupported language server. Confirm fail-soft output.
```

## Done Criteria

This pass is complete when:

- a real `SecurityContextExecutor` adapter exists or integration is explicitly deferred with a clear boundary reason;
- command execution can accept an injected executor;
- `/security-review --enrich` uses real executor when available;
- no-executor runtimes fail soft with clear notes;
- enrichment requests are validated and bounded;
- tests cover adapter, command injection, no-executor, failure, and request validation;
- default deterministic review remains unchanged;
- no mutation, network scan, shell execution, exploit generation, or unbounded call expansion is introduced.

## Follow-Up Passes

After this lands, likely next targets are:

1. TUI findings panel with enrichment status and navigation.
2. Project-level security policy config for thresholds and budgets.
3. Dependency/CVE enrichment for `dependency_review`.
4. Security reviewer agent prompt integration using enriched deterministic receipts.
