# Security Review Productization and LSP Escalation Plan

## Purpose

Productize the completed LSP/security-review substrate so it becomes usable from Codegg workflows, while keeping work split into lanes that can be completed concurrently.

The current state is strong enough to build on:

- LSP integration is effectively complete as a read-only semantic/security context substrate.
- `securityContext` supports presets, risk markers, diagnostics/symbols, bounded call expansion, and provenance/truncation.
- Security review target discovery, prompt generation, evidence-based finding synthesis, and hardening against cross-file evidence bleed are implemented.

This pass should focus on presentation, orchestration, and selective escalation rather than expanding scanner scope.

## Non-Goals

Do not add dependency/CVE lookup in this pass.

Do not add network scanning.

Do not mutate source files.

Do not generate exploit steps or offensive payload guidance.

Do not make call expansion default for all security reviews.

Do not replace the existing LSP/security workflow internals unless necessary.

Do not block normal coding flows with mandatory security review prompts.

## Workstream A — Security Review Workflow Orchestrator

Can be completed independently from UI rendering.

### Goal

Create a single internal orchestration entrypoint that runs the existing security review phases in order and returns a stable report object.

Recommended function:

```rust
pub async fn run_security_review_workflow(
    root: &Path,
    base: Option<&str>,
    options: SecurityReviewWorkflowOptions,
) -> Result<SecurityReviewOutput, String>
```

Recommended options:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityReviewWorkflowOptions {
    pub include_prompts: bool,
    pub include_findings: bool,
    pub run_filename_preflight: bool,
    pub run_content_preflight: bool,
    pub hunk_local_content_preflight: bool,
    pub max_findings: usize,
    pub max_prompts: usize,
}
```

Default options:

```text
include_prompts = true
include_findings = true
run_filename_preflight = true
run_content_preflight = true
hunk_local_content_preflight = true
max_findings = 50
max_prompts = 100
```

Pipeline:

1. `discover_targets_from_diff(root, base)`.
2. Build changed-hunk planning prompts from targets.
3. Run filename preflight.
4. Run hunk-local content preflight.
5. Call `synthesize_evidence_based_findings`.
6. Assemble `SecurityReviewOutput`.

This workstream should not execute `securityContext` yet. That comes in Workstream B.

Acceptance criteria:

- one internal workflow entrypoint exists;
- default options are conservative and bounded;
- output separates prompts, findings, preflight results, and notes;
- no LSP requirement for this orchestrator;
- tests use mocked content/diff inputs where possible.

## Workstream B — Selective `securityContext` Escalation

Can be completed in parallel with Workstreams A/C/D if the interface is defined early.

### Goal

Add a policy that decides when to request richer LSP-backed `securityContext` data for specific high-risk targets or preliminary findings.

Do not call expand-by-default. Escalation must be selective.

Recommended enum:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SecurityContextEscalationLevel {
    None,
    Basic,
    CallDepth1,
    CallDepth2,
}
```

Recommended decision helper:

```rust
pub fn choose_security_context_escalation(
    target: &SecurityReviewTarget,
    finding: Option<&SecurityReviewFinding>,
    prompt: Option<&SecurityReviewPrompt>,
) -> SecurityContextEscalationLevel
```

Initial rules:

- `None`: low-risk changed hunk with no prompt/finding.
- `Basic`: marker prompt with no finding, or dependency review target.
- `CallDepth1`: eligible finding with `Medium+` severity, or target reason is `AuthOrSecretHandling`, `ProcessExecution`, `NetworkBoundary`, or `UnsafeCode`.
- `CallDepth2`: only for `High` severity with `Medium+` confidence and category `auth`, `process`, `unsafe`, `secret`, or `sql`.

Request builder:

```rust
pub fn build_escalated_security_context_request(
    target: &SecurityReviewTarget,
    level: SecurityContextEscalationLevel,
) -> serde_json::Value
```

Mapping:

```text
None: do not request
Basic: call_depth = 0
CallDepth1: call_depth = 1
CallDepth2: call_depth = 2
```

Also set:

```text
max_risk_markers = 80 by default
max_call_nodes = 32 for depth 1
max_call_nodes = 64 for depth 2
```

Acceptance criteria:

- escalation helper is pure and tested;
- no target gets depth > 0 without explicit risk signal;
- request builder respects existing LSP caps;
- docs state escalation is read-only and bounded.

## Workstream C — CLI / Slash Command Surface

Can be completed independently after Workstream A interface exists.

### Goal

Expose security review through a small command surface without forcing TUI integration.

Inspect the current command/slash-command architecture first. Prefer whichever command pattern Codegg already uses.

Possible command names:

```text
/security-review
/security-review --changed
/security-review --base main
/security-review --json
/security-review --prompts-only
/security-review --findings-only
```

Minimum useful behavior:

```text
/security-review --changed
```

should:

- run the internal security review workflow against current diff;
- print summary counts:
  - targets
  - prompts
  - findings
  - preflight failures
- print findings first, then prompts;
- keep output compact by default.

JSON mode should emit `SecurityReviewOutput`.

Acceptance criteria:

- command exists or CLI-compatible internal operation is added;
- default command is read-only;
- output differentiates prompts and findings;
- JSON mode is stable and tested if easy;
- command help mentions findings are heuristic defensive review outputs.

## Workstream D — TUI / Report Rendering Model

Can be completed in parallel with C once the report DTO is stable.

### Goal

Create a renderer/model for prompts vs findings even if full interactive UI integration lands later.

Add display helpers:

```rust
pub fn render_security_review_summary(output: &SecurityReviewOutput) -> String
pub fn render_security_review_findings(output: &SecurityReviewOutput) -> String
pub fn render_security_review_prompts(output: &SecurityReviewOutput) -> String
```

Suggested display sections:

```text
Security Review Summary
- Findings: N
- Review prompts: N
- Preflight checks: pass/fail counts
- Notes: conservative semantics

Findings
[severity/confidence] file:line title
  Evidence: ...
  Recommendation: ...
  Tests: ...

Review Prompts
[file:line] title
  Rationale: ...
```

If there is already a TUI report/view model pattern, add a lightweight DTO instead:

```rust
pub struct SecurityReviewDisplayModel { ... }
```

Acceptance criteria:

- render helpers or display model exists;
- findings and prompts are visually distinct;
- severity/confidence are visible for findings;
- no source mutation/action buttons in this pass;
- tests snapshot or assert key strings.

## Workstream E — Evidence Window Cleanup

Small targeted hardening lane. Can be completed independently.

### Goal

Make `evidence_matches_group` easier to audit.

Current line-window matching is correct in intent but too compact. Replace it with explicit helper functions.

Recommended helpers:

```rust
fn line_bucket_start(line: u32) -> u32 {
    line / 5 * 5
}

fn line_within_group_window(
    evidence_line: u32,
    group_bucket: u32,
    bucket_width: u32,
    radius: u32,
) -> bool {
    let bucket_start = group_bucket;
    let bucket_end = group_bucket + bucket_width.saturating_sub(1);
    let window_start = bucket_start.saturating_sub(radius);
    let window_end = bucket_end.saturating_add(radius);
    evidence_line >= window_start && evidence_line <= window_end
}
```

Then:

```rust
match (line_bucket, evidence.line) {
    (Some(bucket), Some(line)) => line_within_group_window(line, bucket, 5, 5),
    (Some(_), None) => true,
    (None, _) => true,
}
```

Acceptance criteria:

- semantics are clear;
- tests cover below-window, inside-window, above-window, file-level same-file, and different-file rejection;
- no synthesis behavior regression.

## Workstream F — Docs and Skill Updates

Can be completed in parallel; should be finalized after A-D names settle.

Update:

```text
AGENTS.md
architecture/lsp.md
architecture/tool.md
.opencode/skills/security/SKILL.md
```

Document:

- LSP layer is read-only substrate;
- `securityContext` is context, not scanner verdict;
- security review separates prompts from findings;
- findings require evidence beyond markers;
- escalation to call expansion is selective and bounded;
- command surface and output examples;
- no mutation/exploit/network scanning behavior.

Acceptance criteria:

- docs match actual command/function names;
- docs do not imply findings are proof of exploitability;
- docs state when call expansion is used.

## Suggested Parallel Execution Order

### Lane 1 — Orchestrator

Workstream A.

Can be assigned alone. It creates the internal API other lanes consume.

### Lane 2 — Escalation policy

Workstream B.

Can be built with pure unit tests before wiring to actual LSP calls.

### Lane 3 — CLI/report output

Workstreams C and D.

CLI can call the orchestrator; rendering helpers can be developed with fixture outputs.

### Lane 4 — Hardening/docs

Workstreams E and F.

Can run alongside all other lanes; final docs pass should happen after names stabilize.

## Tests

Add tests per workstream.

### Orchestrator

```text
security_workflow_default_options_are_bounded
security_workflow_runs_without_lsp
security_workflow_output_separates_prompts_and_findings
security_workflow_respects_max_findings_and_prompts
```

### Escalation

```text
security_context_escalation_none_for_low_risk_changed_hunk
security_context_escalation_basic_for_marker_prompt
security_context_escalation_depth1_for_medium_finding
security_context_escalation_depth2_for_high_confident_auth_finding
security_context_escalation_never_depth2_for_low_confidence
security_context_request_sets_call_depth_and_caps
```

### CLI / Rendering

```text
security_review_summary_renders_counts
security_review_finding_render_shows_severity_confidence
security_review_prompt_render_has_no_severity
security_review_json_output_serializes
security_review_command_help_mentions_read_only
```

### Evidence window

```text
security_evidence_window_rejects_different_file
security_evidence_window_accepts_same_file_no_line
security_evidence_window_accepts_nearby_line
security_evidence_window_rejects_distant_line
```

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
cargo test -p codegg security_workflow
cargo test -p codegg security_context_escalation
cargo test -p codegg security_review_render
cargo test -p codegg security_evidence_window
rg "run_security_review_workflow|SecurityReviewWorkflowOptions|SecurityContextEscalationLevel|build_escalated_security_context_request" src crates tests architecture AGENTS.md .opencode
rg "/security-review|security-review|Security Review Summary|Review Prompts|Findings" src crates tests architecture AGENTS.md .opencode
```

Manual smoke:

```text
1. Run security review on a small changed Rust file. Expect targets/prompts and no mutation.
2. Run on a change with a risk marker plus changed hunk. Expect finding and prompt separation.
3. Run JSON mode if implemented. Expect serializable `SecurityReviewOutput`.
4. Confirm low-risk targets do not request call expansion.
5. Confirm high-risk eligible finding builds call_depth=1 or call_depth=2 request according to policy.
```

## Done Criteria

This pass is complete when:

- internal security review orchestrator exists;
- selective escalation policy exists and is tested;
- CLI/command or internal callable surface exists for changed-diff review;
- output rendering separates findings from prompts;
- evidence window matching is simplified and tested;
- docs/skills describe the workflow accurately;
- no mutation, exploit workflow, network scan, or unbounded LSP expansion is introduced.

## Follow-Up Passes

After this pass, likely next targets are:

1. Actual LSP-backed securityContext enrichment in the orchestrator.
2. TUI interactive panel with finding navigation.
3. Dependency metadata and CVE context for `dependency_review`.
4. Project policy config for severity thresholds, ignored paths, and review budgets.
5. Provider/model prompt integration for security reviewer agent behavior.
