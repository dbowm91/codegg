# Security Review LSP Enrichment Plan

## Purpose

Add an optional second-stage security review pass that executes bounded, read-only LSP `securityContext` requests for targets selected by the existing escalation policy, then converts the enriched context into additional prompts/evidence and reruns finding synthesis.

Current state this builds on:

- `/security-review` command surface exists.
- `run_security_review_workflow` runs deterministic review without LSP execution.
- `plan_security_context_escalations` produces non-executing escalation recommendations.
- `build_escalated_security_context_request` builds bounded request payloads.
- `prompts_from_security_context` converts risk markers to prompts.
- evidence-based finding synthesis is hardened against cross-file evidence bleed.

This pass should preserve the deterministic first pass and make LSP enrichment explicitly opt-in, bounded, and fail-soft.

## Non-Goals

Do not add dependency/CVE lookup.

Do not add network scanning.

Do not mutate source files.

Do not generate exploit payloads or offensive steps.

Do not make LSP enrichment mandatory for `/security-review`.

Do not run call expansion for every target.

Do not require a live LSP server for unit tests.

Do not collapse prompts and findings into a single output type.

## Desired Flow

The enriched review should run as a two-stage pipeline:

```text
Stage 1: deterministic review
  diff targets
  -> planning prompts
  -> filename/content preflight
  -> evidence-based findings
  -> escalation plan

Stage 2: optional LSP enrichment
  selected escalation plans
  -> bounded securityContext requests
  -> risk markers / diagnostics / call graph context
  -> enriched prompts + structured evidence
  -> second synthesis pass
  -> final output with notes
```

If LSP is unavailable, unsupported, slow, or truncated, Stage 1 output must still be returned with clear notes.

## Phase 1 — Add LSP Enrichment Options

Extend `SecurityReviewWorkflowOptions` or introduce a separate options struct.

Preferred addition:

```rust
pub struct SecurityReviewWorkflowOptions {
    // existing fields...
    pub enable_lsp_enrichment: bool,
    pub max_lsp_enriched_targets: usize,
    pub max_lsp_requests: usize,
    pub lsp_request_timeout_ms: u64,
}
```

Suggested defaults:

```text
enable_lsp_enrichment = false
max_lsp_enriched_targets = 8
max_lsp_requests = 8
lsp_request_timeout_ms = 2500
```

If adding fields to the existing struct is too disruptive, add:

```rust
pub struct SecurityReviewEnrichmentOptions { ... }
```

and thread it through the enriched workflow only.

Acceptance criteria:

- default `/security-review` behavior remains deterministic and no-LSP;
- enrichment must be explicitly enabled;
- limits are hard caps, not advisory comments;
- options serialize cleanly if `SecurityReviewOutput`/command paths need JSON.

## Phase 2 — Define a Mockable SecurityContext Executor Boundary

Do not call the LSP tool directly from synthesis logic. Add a small trait or adapter that can be mocked in tests.

Recommended trait:

```rust
#[async_trait::async_trait]
pub trait SecurityContextExecutor: Send + Sync {
    async fn security_context(
        &self,
        request: serde_json::Value,
    ) -> Result<serde_json::Value, String>;
}
```

If avoiding `async_trait`, use a boxed future type or a simple function closure adapter matching existing project style.

Add a no-op executor for deterministic tests:

```rust
pub struct NoopSecurityContextExecutor;
```

Add a fixture executor for unit tests:

```rust
pub struct FixtureSecurityContextExecutor {
    pub responses: HashMap<PathBuf, serde_json::Value>,
    pub failures: HashMap<PathBuf, String>,
}
```

Actual LSP wiring can be a thin adapter in a later phase if the current TUI/core boundary makes direct invocation awkward.

Acceptance criteria:

- enrichment logic depends on the trait/adapter, not concrete TUI internals;
- tests can return fake `securityContext` JSON;
- executor errors are captured as notes, not panics.

## Phase 3 — Implement Enrichment Runner

Add a function that takes the deterministic output plus escalation plans and returns enrichment artifacts.

Recommended DTOs:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityContextEnrichmentResult {
    pub target: SecurityReviewTarget,
    pub level: SecurityContextEscalationLevel,
    pub request: serde_json::Value,
    pub response: Option<serde_json::Value>,
    pub prompts: Vec<SecurityReviewPrompt>,
    pub evidence: Vec<StructuredSecurityEvidence>,
    pub notes: Vec<String>,
}
```

Recommended function:

```rust
pub async fn run_security_context_enrichment<E: SecurityContextExecutor>(
    output: &SecurityReviewOutput,
    executor: &E,
    options: &SecurityReviewWorkflowOptions,
) -> Vec<SecurityContextEnrichmentResult>
```

Behavior:

1. Call `plan_security_context_escalations(output)`.
2. Filter to `level != None`.
3. Sort by priority:
   - `CallDepth2` first;
   - `CallDepth1` next;
   - `Basic` last;
   - findings before prompts;
   - high severity/confidence first.
4. Apply `max_lsp_enriched_targets` and `max_lsp_requests`.
5. Execute each request through `SecurityContextExecutor` with timeout.
6. Convert response to prompts/evidence.
7. Record failures/truncation as notes.

Timeout wrapper:

```rust
tokio::time::timeout(Duration::from_millis(options.lsp_request_timeout_ms), ...)
```

Acceptance criteria:

- no request executes for `None` plans;
- request count is capped;
- timeout/failure does not fail the whole review;
- returned results include notes for failures/truncation;
- unit tests use fixture executor.

## Phase 4 — Convert Enriched Context to Structured Evidence

`prompts_from_security_context` already converts risk markers to prompts. Add a companion conversion that emits structured evidence.

Recommended function:

```rust
pub fn evidence_from_security_context(
    target: &SecurityReviewTarget,
    context_json: &serde_json::Value,
) -> Vec<StructuredSecurityEvidence>
```

Evidence sources:

### Risk markers

Map to:

```rust
SecurityEvidenceKind::RiskMarker
```

File path and line should come from marker fields when present, falling back to target path/line.

Accept both:

```text
file
file_path
```

### Diagnostics

If the `securityContext` response includes diagnostics or a diagnostic summary, convert relevant entries to:

```rust
SecurityEvidenceKind::Diagnostic
```

Keep diagnostics same-file scoped.

### Call graph / call path

If call expansion returns inbound/outbound callers/callees or a call graph summary, convert non-empty reachable call context to:

```rust
SecurityEvidenceKind::CallPath
```

Do not include huge call graph details in evidence. Use a compact summary such as:

```text
securityContext call expansion returned N nodes and M edges at depth D
```

### Truncation

If response has `truncated=true` or nested truncation flags, add:

```rust
SecurityEvidenceKind::TruncationNotice
```

Acceptance criteria:

- evidence is file-scoped;
- evidence does not contain large raw JSON payloads;
- risk marker conversion accepts `file` and `file_path`;
- truncation reduces later confidence via existing synthesis behavior.

## Phase 5 — Add Enriched Review Workflow

Keep existing `run_security_review_workflow` deterministic and no-LSP.

Add a new explicit function:

```rust
pub async fn run_security_review_workflow_with_lsp_enrichment<E: SecurityContextExecutor>(
    root: &Path,
    base: Option<&str>,
    options: SecurityReviewWorkflowOptions,
    executor: &E,
) -> Result<SecurityReviewOutput, String>
```

Behavior:

1. Run `run_security_review_workflow(root, base, options_without_enrichment)`.
2. If `enable_lsp_enrichment` is false, return stage-1 output.
3. Run `run_security_context_enrichment`.
4. Merge enriched prompts into stage-1 prompts.
5. Convert enriched evidence into a synthetic evidence source for synthesis.
6. Rerun `synthesize_evidence_based_findings` with:
   - original targets;
   - original remaining prompts + enriched prompts;
   - original preflight;
   - enriched evidence support.

If `synthesize_evidence_based_findings` cannot accept external evidence yet, add a narrow overload:

```rust
pub fn synthesize_evidence_based_findings_with_extra_evidence(
    targets: &[SecurityReviewTarget],
    prompts: &[SecurityReviewPrompt],
    preflight: &[SecurityPreflightResult],
    extra_evidence: &[StructuredSecurityEvidence],
) -> (Vec<SecurityReviewFinding>, Vec<SecurityReviewPrompt>)
```

Acceptance criteria:

- deterministic workflow remains unchanged;
- enriched workflow is opt-in and explicit;
- second-pass synthesis can use call-path/diagnostic/risk-marker evidence;
- failed enrichment returns stage-1 output plus notes.

## Phase 6 — Add Command Flag

Add a command flag to opt into enrichment.

Preferred names:

```text
/security-review --enrich
/security-review --lsp
```

Pick one canonical flag. Suggested canonical flag:

```text
--enrich
```

Add optional caps:

```text
--max-enriched-targets N
--lsp-timeout-ms N
```

Behavior:

- without `--enrich`, current deterministic command behavior remains unchanged;
- with `--enrich`, use the enriched workflow if an LSP executor is available;
- if no LSP executor is available in the current runtime path, return deterministic output plus note:

```text
LSP enrichment requested but no securityContext executor is available in this runtime.
```

Acceptance criteria:

- command parser supports `--enrich`;
- deterministic default does not change;
- no-LSP runtime fails soft;
- docs mention enrichment is read-only, bounded, and optional.

## Phase 7 — Wire Actual Executor Where Safe

Inspect existing LSP tool operation boundary:

```bash
rg "securityContext" src crates -g '*.rs'
rg "build_security_context" src crates -g '*.rs'
rg "CoreRequest|ToolRequest|Lsp" src crates -g '*.rs'
```

Add the real executor only if there is a clean internal call path that does not couple workflow code directly to TUI state.

Preferred shape:

```rust
pub struct LspToolSecurityContextExecutor { ... }
```

or a core/client adapter depending on existing architecture.

Rules:

- executor is read-only;
- executor must honor request caps;
- executor must not mutate files;
- unsupported language server returns a note, not failure of whole review;
- no direct TUI dependency inside security workflow modules.

If wiring is not clean, stop at fixture/no-op executor and document that real executor integration is next.

Acceptance criteria:

- either real executor is wired safely, or clearly deferred with no dead code;
- workflow module remains testable without live LSP;
- command behavior is clear when enrichment is unavailable.

## Phase 8 — Tests

Add tests with fixture executor. Do not require a live language server.

### Executor and runner tests

```text
security_enrichment_skips_none_plans
security_enrichment_caps_request_count
security_enrichment_records_executor_failure_as_note
security_enrichment_records_timeout_as_note
security_enrichment_converts_marker_response_to_prompt
security_enrichment_converts_call_graph_to_call_path_evidence
security_enrichment_converts_diagnostic_to_diagnostic_evidence
security_enrichment_converts_truncation_to_truncation_notice
```

### Synthesis tests

```text
security_enriched_call_path_plus_marker_promotes_finding
security_enriched_diagnostic_plus_marker_promotes_finding
security_enriched_marker_only_still_not_finding_without_support
security_enriched_different_file_evidence_does_not_promote
```

### Command tests

```text
security_review_command_enrich_flag_parses
security_review_command_enrich_without_executor_fails_soft
security_review_command_default_does_not_enrich
security_review_command_enrich_respects_caps
```

### Real executor tests, if wired

Use mocked operation boundary, not live server:

```text
security_context_executor_builds_expected_request
security_context_executor_maps_tool_error_to_string
```

Acceptance criteria:

- no test requires network or live LSP;
- timeout/failure paths are tested;
- marker-only invariant remains intact;
- cross-file evidence isolation remains intact.

## Phase 9 — Documentation Updates

Update:

```text
AGENTS.md
architecture/lsp.md
architecture/tool.md
.opencode/skills/security/SKILL.md
README.md if command usage is documented there
```

Document:

- `/security-review` remains deterministic by default;
- `--enrich` runs optional LSP-backed `securityContext` enrichment;
- enrichment is read-only and bounded;
- no enrichment failure blocks the deterministic review;
- escalation plans are policy recommendations unless executed by `--enrich`;
- `CallDepth2` is reserved for high-severity, medium/high-confidence cases;
- findings remain heuristic defensive review outputs, not proof of exploitability.

Example docs:

```text
/security-review --changed
/security-review --changed --enrich
/security-review --base main --enrich --max-enriched-targets 4
/security-review --changed --json --enrich
```

Acceptance criteria:

- docs match actual flag names;
- docs do not imply LSP enrichment is required;
- docs preserve no-mutation/no-exploit semantics.

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
cargo test -p codegg security_enrichment
cargo test -p codegg security_context_executor
cargo test -p codegg security_review_command_enrich
cargo test -p codegg security_enriched
rg "SecurityContextExecutor|SecurityContextEnrichmentResult|run_security_context_enrichment|run_security_review_workflow_with_lsp_enrichment|--enrich" src crates tests architecture AGENTS.md .opencode README.md
rg "enable_lsp_enrichment|max_lsp_enriched_targets|lsp_request_timeout_ms" src crates tests
```

Manual smoke:

```text
1. Run /security-review --changed. Confirm deterministic output is unchanged.
2. Run /security-review --changed --enrich in a runtime without LSP executor. Confirm deterministic output plus fail-soft note.
3. Run enriched workflow with fixture executor returning a risk marker. Confirm enriched prompt appears.
4. Run fixture executor returning call graph summary. Confirm CallPath evidence can support eligible finding.
5. Run fixture executor failure. Confirm review still returns stage-1 output and notes the failure.
```

## Done Criteria

This pass is complete when:

- deterministic security review remains the default;
- optional enrichment options and command flag exist;
- enrichment runner executes only non-`None` bounded escalation plans;
- responses convert into prompts and structured evidence;
- second-pass synthesis can use enriched evidence;
- failures/timeouts/truncation are recorded as notes;
- unit tests use mocked executors, not live LSP;
- docs explain optional read-only enrichment accurately;
- no mutation, network scan, exploit generation, or unbounded call expansion is introduced.

## Follow-Up Passes

After this lands, likely next targets are:

1. Interactive TUI panel with finding navigation and enrichment status.
2. Dependency/CVE context for `dependency_review`.
3. Project policy config for security budgets, ignored paths, and severity thresholds.
4. Security reviewer agent prompt integration using deterministic review receipts.
