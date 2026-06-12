# Security Review CLI and Root-Path Follow-Up Plan

## Purpose

Finish the productization pass by wiring the internal security-review workflow to an actual user-facing command surface, fixing the repo-root content read bug, and keeping the workflow maintainable as `src/security/workflow.rs` grows.

The internal substrate is mostly in place:

- `run_security_review_workflow` exists.
- `SecurityReviewWorkflowOptions` exists.
- selective `securityContext` escalation helpers exist.
- summary/findings/prompts render helpers exist.
- prompt-only synthesis was separated from evidence-based synthesis.
- evidence-window matching was cleaned up.

Remaining productization gaps:

1. no confirmed CLI/slash-command entrypoint for running security review;
2. `run_security_review_workflow` reads content preflight files through `std::fs::read_to_string(p)` instead of `root.join(p)`;
3. `src/security/workflow.rs` is now too broad and should be lightly modularized if that can be done without churn;
4. actual LSP-backed `securityContext` enrichment is still policy-only, not orchestrator-wired.

## Non-Goals

Do not add dependency/CVE lookup.

Do not add network scanning.

Do not mutate files.

Do not add exploit/offensive payload guidance.

Do not make call expansion default for all targets.

Do not redesign the LSP layer.

Do not require TUI rendering before CLI/text output works.

## Phase 1 — Fix Repo-Root Content Preflight Reads

`discover_targets_from_diff(root, base)` produces repo-relative target paths. The orchestrator currently calls content preflight with loaders like:

```rust
std::fs::read_to_string(p).ok()
```

This only works when process cwd is the repo root. Update both content preflight branches to join through `root`:

```rust
run_content_preflight_checks_for_targets(&targets, |p| {
    std::fs::read_to_string(root.join(p)).ok()
})
```

and:

```rust
run_content_preflight_checks(&targets, |p| {
    std::fs::read_to_string(root.join(p)).ok()
})
```

Acceptance criteria:

- content preflight works when Codegg is launched outside the repository root;
- tests cover repo-relative target paths with a temp root;
- no absolute-path leakage is introduced into target paths.

## Phase 2 — Add a Thin Security Review Command Surface

Inspect existing command/slash-command architecture first. Reuse existing patterns instead of inventing a separate command system.

Search targets:

```bash
rg "slash|command|Command|/" src crates -g '*.rs'
rg "json|render|output" src crates -g '*.rs'
rg "apply_patch|tool" src crates -g '*.rs'
```

Add a command equivalent to:

```text
/security-review
/security-review --changed
/security-review --base <ref>
/security-review --json
/security-review --prompts-only
/security-review --findings-only
```

Minimum viable command:

```text
/security-review --changed
```

Behavior:

1. Resolve repo root using the same helper Codegg uses for git/tool operations.
2. Run `run_security_review_workflow(root, base, options)`.
3. Render compact text output by default:
   - summary;
   - findings;
   - prompts.
4. Emit JSON when `--json` is present.
5. Stay read-only.

Recommended flag mapping:

```text
--base <ref>        -> base: Some(ref)
--changed          -> current diff against default base behavior
--json             -> serialize SecurityReviewOutput
--prompts-only     -> include_findings=false
--findings-only    -> include_prompts=false
--no-content       -> run_content_preflight=false
--no-filename      -> run_filename_preflight=false
--max-findings N   -> max_findings=N
--max-prompts N    -> max_prompts=N
```

If full argument parsing is too much for this pass, implement only:

```text
/security-review
/security-review --json
```

and leave TODOs for the rest.

Acceptance criteria:

- a user/agent can invoke security review through the existing Codegg command surface;
- command is read-only;
- default output separates findings and prompts;
- JSON output is stable if implemented;
- command errors are clear when not in a git repo or diff discovery fails.

## Phase 3 — Add Command Tests or Command-Adjacent Tests

Prefer tests at the parser/handler boundary.

If direct command integration is hard to unit test, extract a command handler:

```rust
pub async fn run_security_review_command(
    root: &Path,
    args: SecurityReviewCommandArgs,
) -> Result<String, String>
```

and test this helper with mocked or fixture root inputs.

Required tests:

```text
security_review_command_default_is_read_only
security_review_command_json_serializes_output
security_review_command_prompts_only_clears_findings
security_review_command_findings_only_clears_prompts
security_review_command_uses_root_for_content_reads
security_review_command_reports_diff_errors
```

Acceptance criteria:

- command behavior is pinned;
- root-path fix is tested at command or workflow level;
- no live LSP dependency is required.

## Phase 4 — Wire Selective Escalation as Policy Output, Not Execution

Do not execute LSP `securityContext` from the orchestrator yet unless the existing LSP client boundary is trivial to call safely.

Instead, expose escalation recommendations in a pure, testable way.

Add a DTO:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityContextEscalationPlan {
    pub target: SecurityReviewTarget,
    pub level: SecurityContextEscalationLevel,
    pub request: Option<serde_json::Value>,
    pub reason: String,
}
```

Add helper:

```rust
pub fn plan_security_context_escalations(
    output: &SecurityReviewOutput,
) -> Vec<SecurityContextEscalationPlan>
```

Behavior:

- map findings/prompts back to targets by file/line bucket;
- use `choose_security_context_escalation`;
- build request only when level != `None`;
- do not execute it.

Add to `SecurityReviewOutput` only if non-breaking. If not, expose separately.

Acceptance criteria:

- escalation policy is visible to callers;
- no LSP execution is introduced in this pass;
- depth/caps are bounded;
- tests verify high-risk findings get depth 1/2 plans and low-risk targets do not.

## Phase 5 — Light Module Split

`src/security/workflow.rs` now contains DTOs, diff parsing, preflight, synthesis, orchestration, escalation, rendering, and tests. Split only if low-risk.

Preferred shape:

```text
src/security/workflow.rs          # public facade and orchestrator
src/security/workflow/types.rs    # DTOs/enums
src/security/workflow/diff.rs     # diff parsing and target discovery helpers
src/security/workflow/preflight.rs
src/security/workflow/synthesis.rs
src/security/workflow/escalation.rs
src/security/workflow/render.rs
```

Alternative if Rust module naming conflicts are annoying:

```text
src/security/workflow_types.rs
src/security/workflow_diff.rs
src/security/workflow_preflight.rs
src/security/workflow_synthesis.rs
src/security/workflow_escalation.rs
src/security/workflow_render.rs
```

Rules:

- preserve existing public exports from `src/security/workflow.rs`;
- move tests with implementation where practical;
- avoid large behavior changes;
- do not split if it risks destabilizing the command work.

Acceptance criteria:

- public API remains compatible;
- `cargo test -p codegg security_` still passes;
- file size becomes easier to navigate;
- docs references do not break.

## Phase 6 — Docs and Skill Updates

Update docs after command names settle:

```text
AGENTS.md
architecture/tool.md
architecture/lsp.md
.opencode/skills/security/SKILL.md
```

Document:

```markdown
/security-review runs a read-only security review over changed files. It separates findings from review prompts. Findings are heuristic defensive review outputs, not proof of exploitability. securityContext escalation is planned/bounded and not executed unless explicitly wired by a later pass.
```

Include examples:

```text
/security-review --changed
/security-review --changed --json
/security-review --base main --findings-only
```

Acceptance criteria:

- docs mention the actual command name and options;
- docs preserve marker-not-finding semantics;
- docs state content preflight uses repo-root-relative reads;
- docs state no mutation/network/exploit behavior.

## Tests

Add or update tests.

### Root-path tests

```text
security_workflow_content_preflight_reads_from_root
security_workflow_content_preflight_does_not_depend_on_cwd
```

### Command tests

```text
security_review_command_parses_default
security_review_command_parses_json
security_review_command_parses_prompts_only
security_review_command_parses_findings_only
security_review_command_renders_summary_findings_prompts
```

### Escalation-plan tests

```text
security_context_escalation_plan_none_for_low_risk
security_context_escalation_plan_basic_for_prompt
security_context_escalation_plan_depth1_for_medium_finding
security_context_escalation_plan_depth2_for_high_confident_auth
security_context_escalation_plan_has_bounded_request_caps
```

### Module split regression

```text
security_workflow_public_exports_still_available
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
cargo test -p codegg security_review_command
cargo test -p codegg security_context_escalation_plan
cargo test -p codegg security_workflow_content_preflight_reads_from_root
rg "security-review|run_security_review_command|SecurityReviewCommandArgs|run_security_review_workflow|plan_security_context_escalations" src crates tests architecture AGENTS.md .opencode
rg "read_to_string\(p\)|std::fs::read_to_string\(p\)" src/security src crates
```

Manual smoke:

```text
1. Launch Codegg from outside repo root and run security review against a repo path. Content preflight should still read files.
2. Run /security-review --changed. Expect summary/findings/prompts, no mutation.
3. Run /security-review --changed --json. Expect valid SecurityReviewOutput JSON.
4. Run /security-review --findings-only. Expect no prompt section.
5. Verify low-risk changed hunk does not produce call-depth escalation.
```

## Done Criteria

This pass is complete when:

- repo-root content preflight bug is fixed and tested;
- a real security-review command or command-equivalent handler exists;
- default command output separates findings and prompts;
- JSON output exists or is explicitly deferred in docs/tests;
- escalation plans are available without executing LSP;
- workflow module is either lightly split or explicitly left intact with rationale;
- docs/skills reflect the actual user-facing behavior;
- no mutation, network scanning, exploit generation, or unbounded LSP expansion is introduced.

## Follow-Up Passes

After this lands:

1. Wire actual LSP-backed `securityContext` enrichment for planned escalations.
2. Add interactive TUI panel with finding navigation.
3. Add dependency/CVE context for `dependency_review`.
4. Add project policy configuration for thresholds, ignored paths, and budgets.
