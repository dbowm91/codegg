# Security Agent Workflow Plan

## Purpose

Add a first-class security-agent workflow that consumes the existing LSP-backed `securityContext` capability and deterministic local checks to produce evidence-based review findings.

The goal is not to turn Codegg into a pentest tool. The workflow should help the security agent review code changes and high-risk code paths with structured context, clear evidence, and conservative findings.

The current LSP context stack is ready for this:

- `securityContext` is read-only, bounded, and preset-aware.
- Risk markers are deterministic review prompts, not vulnerability verdicts.
- Optional bounded call expansion is default-off and strictly capped.
- Overlay diagnostics can evaluate proposed full content or single-file patches without writing to disk.
- Output truncation and provenance are coherent.

## High-Level Workflow

The security agent should follow this loop:

```text
1. Identify changed files and changed hunks.
2. Classify each file/use-case into a security preset.
3. Run deterministic preflight checks where available.
4. Request securityContext around changed hunks and high-risk symbols.
5. Correlate risk markers, diagnostics, symbols, definitions/references, and bounded call expansion.
6. Produce findings only when there is concrete evidence.
7. Distinguish review prompts from confirmed findings.
8. Suggest minimal mitigations or tests.
```

## Non-Goals

Do not add exploit generation.

Do not add offensive workflow automation.

Do not add autonomous network interaction.

Do not add command execution beyond existing approved local test/check tooling.

Do not mutate files in the security-review phase.

Do not treat risk markers as findings.

Do not block normal coding workflows with mandatory security scans.

Do not add dependency/CVE lookup in this pass.

Do not add taint analysis in this pass.

## Terminology

### Risk marker

A deterministic pattern/context hit from `securityContext`. A risk marker says “review this code path.” It does not say “this is vulnerable.”

### Finding

A security review conclusion with concrete evidence, severity, affected code, reasoning, and recommended mitigation.

### Evidence

Specific code locations, diagnostics, call relationships, or deterministic tool output that supports a finding.

### Confidence

A conservative confidence label:

```text
low, medium, high
```

Confidence is about evidence quality, not impact.

## Phase 1 — Add Security Agent Workflow Types

Locate the current agent/profile/task orchestration structure. Search for existing agent role/profile definitions before implementing.

Likely places to inspect:

```text
src/agent*
src/config*
src/tool*
crates/*/src
.opencode/skills
AGENTS.md
```

Add internal workflow structures where they fit the current architecture.

Recommended types:

```rust
struct SecurityReviewTarget {
    file_path: PathBuf,
    line: Option<u32>,
    column: Option<u32>,
    preset: SecurityPresetChoice,
    reason: SecurityTargetReason,
}

enum SecurityTargetReason {
    ChangedHunk,
    RiskMarker,
    PublicBoundary,
    UnsafeCode,
    ProcessExecution,
    FilesystemAccess,
    NetworkBoundary,
    AuthOrSecretHandling,
}

struct SecurityReviewFinding {
    severity: SecuritySeverity,
    confidence: SecurityConfidence,
    title: String,
    file_path: PathBuf,
    line: Option<u32>,
    evidence: Vec<SecurityEvidence>,
    reasoning: String,
    recommendation: String,
    tests: Vec<String>,
}

enum SecuritySeverity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

enum SecurityConfidence {
    Low,
    Medium,
    High,
}
```

Keep these internal. Do not expose stable API until the workflow settles.

Acceptance criteria:

- workflow data model distinguishes targets, markers, and findings;
- severity and confidence are separate;
- finding evidence is structured.

## Phase 2 — Changed-Hunk Target Discovery

Add or reuse a Git diff parser for changed files and hunks.

Preferred source order:

1. existing repo diff/change tracking if present;
2. `git diff --unified=0` through existing safe shell/check infrastructure if allowed;
3. internal diff parser if already present for patch tooling.

The workflow should identify:

```rust
struct ChangedHunk {
    file_path: PathBuf,
    old_start: u32,
    old_count: u32,
    new_start: u32,
    new_count: u32,
}
```

Target selection:

- for each changed hunk, request `securityContext` centered near `new_start`;
- if a hunk has no line count, use file-level context with no position;
- if symbol lookup can locate enclosing function cheaply, prefer the symbol start line.

Acceptance criteria:

- changed files and changed hunks can be collected without network access;
- binary/deleted files are skipped with notes;
- generated/vendor paths can be excluded by default.

## Phase 3 — Preset Selection Heuristics

Add a lightweight mapping from file path/content hints to `security_preset`.

Recommended initial heuristic:

```text
unsafe_review:
  contains unsafe, ffi, raw pointer, atomics, concurrency primitives

web_backend:
  path or symbols suggest routes, handlers, auth, middleware, session, jwt, cookie, sql, request parsing

rust_cli:
  path/symbols suggest cli, command, process, filesystem, config, local automation

rust_server:
  default for Rust service files, networking, MCP server, daemon, WAF/proxy code

dependency_review:
  Cargo.toml, Cargo.lock, build.rs, package metadata, dependency-loading code
```

Keep it deterministic and overrideable.

Potential config:

```toml
[security.presets]
default = "rust_server"
path_overrides = [
  { glob = "**/Cargo.toml", preset = "dependency_review" },
  { glob = "**/build.rs", preset = "dependency_review" },
]
```

Do not require config in the first pass if this would add too much churn. Hardcoded heuristics with docs are acceptable initially.

Acceptance criteria:

- target discovery assigns one of the existing `securityContext` presets;
- user/config override path is designed, even if minimal;
- heuristics are deterministic and tested.

## Phase 4 — SecurityContext Request Strategy

For each target, request `securityContext` with bounded settings.

Default request shape for changed hunks:

```json
{
  "operation": "securityContext",
  "file_path": "...",
  "line": <hunk or symbol line>,
  "column": 1,
  "security_preset": "...",
  "call_depth": 0,
  "max_risk_markers": 80
}
```

Escalation strategy:

Only request call expansion when evidence suggests it is useful:

```text
call_depth = 1 when risk markers include auth/network/process/unsafe or changed hunk is in a public boundary
call_depth = 2 only when user explicitly requests deep security review or review budget allows it
```

Do not default to `call_depth=2`.

For patch review:

- if reviewing proposed patch content, pass a single-file patch to `securityContext` overlay;
- never write proposed content to disk during review.

Acceptance criteria:

- first-pass review is cheap and bounded;
- expansion is opt-in/escalated, not automatic for every file;
- overlay review preserves no-write contract.

## Phase 5 — Deterministic Preflight Checks

Identify existing deterministic checks available in Codegg before adding new tools.

Possible safe checks:

```text
cargo check
cargo clippy
cargo test targeted tests
cargo audit / cargo deny only if already integrated or explicitly configured
rg-based secret-pattern scan over changed lines
rg-based unsafe/process/fs/network pattern scan over changed lines
```

Do not hard-require tools that are not present.

Initial recommendation:

- add a deterministic changed-line grep layer only;
- optionally invoke existing project check/test tooling if the harness already supports it;
- defer external dependency scanners.

Potential preflight DTO:

```rust
struct SecurityPreflightResult {
    check_name: String,
    status: PreflightStatus,
    evidence: Vec<SecurityEvidence>,
    notes: Vec<String>,
}
```

Acceptance criteria:

- preflight is deterministic;
- failures are nonfatal review inputs;
- no network or external scanner dependency is introduced by default.

## Phase 6 — Finding Synthesis Rules

The security agent should not emit a finding from a risk marker alone.

Minimum finding evidence:

At least one of:

```text
1. Risk marker + changed code + plausible data/control flow explanation.
2. Diagnostic/error + security-relevant code path.
3. Call expansion shows changed code reachable from a public/auth/network boundary.
4. Deterministic preflight check identifies concrete issue in changed lines.
5. Manual code reasoning over excerpt identifies an actual unsafe behavior.
```

Finding format:

```markdown
### [Severity] Title

- Confidence: low|medium|high
- File: path:line
- Evidence:
  - path:line — specific code/context
  - securityContext marker: category/label/rationale
  - call path: A -> B -> C, if available
- Reasoning: concise explanation of the issue
- Recommendation: minimal fix
- Suggested tests: specific test cases or checks
```

Rules:

- mark “needs review” items separately from findings;
- do not inflate severity without exploitability evidence;
- avoid claiming confirmed vulnerability without a concrete path;
- call out missing context/truncation.

Acceptance criteria:

- findings are evidence-based;
- risk markers remain separate from findings;
- output format is stable and parseable enough for TUI/CLI display.

## Phase 7 — Agent Prompt/Profile Integration

Add or update a security-agent prompt/profile.

The prompt should instruct the agent to:

- use `securityContext` for changed hunks and risky symbols;
- use presets based on target type;
- request call expansion only when useful;
- treat risk markers as review prompts;
- produce evidence-based findings;
- avoid exploit instructions and offensive automation;
- suggest defensive fixes/tests only.

Suggested prompt fragment:

```text
You are a defensive code security reviewer. Use deterministic tools and securityContext packets to gather evidence. Risk markers are review prompts, not findings. Emit findings only when evidence supports a concrete issue. Prefer minimal mitigations and tests. Do not provide exploit steps or offensive automation.
```

Acceptance criteria:

- security profile exists or is updated;
- prompt explicitly distinguishes markers from findings;
- prompt requires evidence and defensive recommendations.

## Phase 8 — CLI/TUI Workflow Surface

Expose a minimal way to invoke the workflow.

Possible command shapes, depending on current architecture:

```text
/security-review
/security-review --changed
/security-review --file src/server/auth.rs
/security-review --preset rust_server
/security-review --deep
```

Initial minimal surface:

```text
/security-review --changed
```

Behavior:

- discover changed hunks;
- build targets;
- gather contexts;
- return findings and review prompts;
- do not mutate files.

If CLI/TUI command plumbing is currently not ready, implement as an internal workflow callable from agent orchestration and document the future command.

Acceptance criteria:

- workflow can be invoked internally or from an existing command path;
- output is readable in TUI/CLI;
- no mandatory review blocks normal editing.

## Phase 9 — Tests

Add hermetic tests where possible.

Target discovery tests:

```text
security_review_extracts_changed_hunks
security_review_skips_deleted_or_binary_files
security_review_excludes_vendor_generated_paths
```

Preset heuristic tests:

```text
security_review_selects_dependency_review_for_cargo_toml
security_review_selects_unsafe_review_for_unsafe_code
security_review_selects_web_backend_for_handler_auth_paths
security_review_defaults_to_rust_server_for_service_code
```

Finding synthesis tests:

```text
security_review_does_not_emit_finding_for_marker_only
security_review_emits_review_prompt_for_marker_only
security_review_emits_finding_with_marker_and_flow_evidence
security_review_includes_truncation_note_when_context_truncated
```

Prompt/profile tests if prompts are snapshot-tested:

```text
security_agent_prompt_mentions_risk_markers_are_not_findings
security_agent_prompt_requires_evidence
security_agent_prompt_limits_to_defensive_recommendations
```

Acceptance criteria:

- target discovery and preset selection are tested;
- marker-only behavior is tested;
- finding format is tested;
- no live LSP server is required for most tests.

## Phase 10 — Documentation

Update:

```text
architecture/lsp.md
architecture/tool.md
architecture/agents.md if present
.opencode/skills/lsp/SKILL.md
AGENTS.md
```

Document:

- security review workflow;
- target discovery;
- preset selection;
- when call expansion is used;
- marker vs finding distinction;
- no mutation/no exploit/no network behavior;
- output format.

Suggested docs section:

```markdown
### Security agent workflow

The security agent uses `securityContext` as evidence-gathering input for defensive code review. It first targets changed hunks, chooses a preset, gathers bounded context, and only emits findings when evidence supports a concrete issue. Risk markers are triage prompts, not findings.
```

Acceptance criteria:

- docs explain safe usage and boundaries;
- docs make workflow understandable to future contributors;
- no docs imply offensive automation or vulnerability confirmation from markers alone.

## Phase 11 — Validation Commands

Run:

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Targeted searches:

```bash
rg "security-review|security_review|SecurityReview|SecurityReviewFinding|SecurityReviewTarget" src crates tests architecture .opencode AGENTS.md
rg "risk markers are.*not findings|review prompts|evidence-based" src crates tests architecture .opencode AGENTS.md
rg "call_depth|security_preset|securityContext" src crates tests architecture .opencode AGENTS.md
rg "exploit|payload|offensive|attack" src crates tests architecture .opencode AGENTS.md
```

Manual smoke:

```text
1. Run security review on a small changed Rust file with no risky code; expect no findings.
2. Run on a changed file containing process/file/network/auth code; expect review prompts and contexts.
3. Run with a known defensive bug fixture; expect one evidence-based finding.
4. Confirm no files are modified by the review workflow.
5. Confirm call_depth is used only under explicit/deep review conditions.
```

## Done Criteria

This pass is complete when:

- changed-hunk/security target discovery exists;
- deterministic preset selection exists;
- securityContext request strategy is implemented;
- marker-only evidence does not become a finding;
- finding output has severity, confidence, evidence, reasoning, recommendation, and tests;
- security-agent prompt/profile is updated;
- workflow is callable internally or through an existing command path;
- tests cover target discovery, preset selection, and marker-vs-finding behavior;
- docs explain the workflow and safety boundaries;
- no mutation, offensive automation, or network scanning is introduced.

## Follow-Up Passes

After this workflow lands:

1. Add dependency metadata/CVE context for `dependency_review`.
2. Add optional security review report export.
3. Add TUI affordances for findings vs review prompts.
4. Add configurable review budgets for call expansion and preflight checks.
5. Add project policy configuration for severity thresholds and ignored paths.
