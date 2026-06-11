# Security Agent Evidence-Based Finding Synthesis Plan

## Purpose

Implement the next security-agent workflow phase: convert review prompts and deterministic evidence into conservative, evidence-based security findings.

The current vertical slice is ready:

- changed-hunk parsing is implemented;
- target discovery and preset selection are implemented;
- securityContext request payload construction is implemented;
- risk markers become review prompts, not findings;
- filename-only preflight checks are honestly named;
- future finding scaffolding exists but is inert.

This pass should activate finding synthesis under strict rules. The core invariant remains: a risk marker alone is never enough to produce a confirmed finding.

## Non-Goals

Do not generate exploit steps.

Do not add offensive automation.

Do not run network scans.

Do not mutate files.

Do not add dependency/CVE lookup yet.

Do not add full taint analysis.

Do not enable `securityContext` call expansion by default for all targets.

Do not claim confirmed vulnerabilities without concrete evidence.

## Finding Model

Replace stringly-typed future fields with enums and structured evidence.

Recommended definitions in `src/security/workflow.rs` or a small sibling module if the file is too large:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SecuritySeverity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SecurityConfidence {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SecurityEvidenceKind {
    ChangedHunk,
    RiskMarker,
    Diagnostic,
    CallPath,
    Preflight,
    CodeReasoning,
    TruncationNotice,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityEvidence {
    pub kind: SecurityEvidenceKind,
    pub file_path: Option<PathBuf>,
    pub line: Option<u32>,
    pub summary: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityReviewFinding {
    pub severity: SecuritySeverity,
    pub confidence: SecurityConfidence,
    pub title: String,
    pub file_path: PathBuf,
    pub line: Option<u32>,
    pub category: Option<String>,
    pub evidence: Vec<SecurityEvidence>,
    pub reasoning: String,
    pub recommendation: String,
    pub tests: Vec<String>,
}
```

If changing existing structs breaks too many tests, add a new `EvidenceBasedSecurityFinding` first and migrate in a cleanup pass.

Acceptance criteria:

- severity and confidence are enums, not ad hoc strings;
- evidence has kind/location/summary/detail;
- findings remain serializable for future TUI/CLI display.

## Finding Eligibility Rules

Implement an explicit gate:

```rust
pub fn marker_only_is_finding_eligible(...) -> bool { false }
```

Then a real eligibility helper:

```rust
pub fn is_finding_eligible(evidence: &[SecurityEvidence]) -> bool {
    let has_marker = evidence.iter().any(|e| e.kind == SecurityEvidenceKind::RiskMarker);
    let has_changed_hunk = evidence.iter().any(|e| e.kind == SecurityEvidenceKind::ChangedHunk);
    let has_preflight_fail = evidence.iter().any(|e| e.kind == SecurityEvidenceKind::Preflight);
    let has_call_path = evidence.iter().any(|e| e.kind == SecurityEvidenceKind::CallPath);
    let has_diagnostic = evidence.iter().any(|e| e.kind == SecurityEvidenceKind::Diagnostic);
    let has_reasoning = evidence.iter().any(|e| e.kind == SecurityEvidenceKind::CodeReasoning);

    (has_marker && (has_changed_hunk || has_preflight_fail || has_call_path || has_diagnostic || has_reasoning))
        || (has_preflight_fail && has_changed_hunk)
        || (has_reasoning && has_changed_hunk)
}
```

Rules:

- marker-only => prompt only;
- preflight filename hint alone => prompt only;
- changed hunk alone => prompt only;
- marker + changed hunk => eligible only for low/medium confidence unless reasoning is strong;
- marker + call path to public/auth/network boundary => eligible;
- deterministic content preflight failure + changed line => eligible;
- truncation reduces confidence or adds note.

Acceptance criteria:

- marker-only tests still produce zero findings;
- eligibility requires at least two meaningful evidence dimensions or explicit code reasoning;
- all finding paths go through the gate.

## Evidence Collection From Existing Outputs

Add converters that turn current workflow/context data into `SecurityEvidence`.

### From target

```rust
pub fn evidence_from_target(target: &SecurityReviewTarget) -> SecurityEvidence
```

Kind: `ChangedHunk` when target reason is `ChangedHunk`; otherwise use `CodeReasoning` or target-specific classification if needed.

### From risk marker prompt/context

```rust
pub fn evidence_from_review_prompt(prompt: &SecurityReviewPrompt) -> Vec<SecurityEvidence>
```

For prompts from `source: securityContext.risk_marker`, emit `RiskMarker` evidence.

For prompts from `source: changed_hunk`, emit `ChangedHunk` evidence.

### From preflight

Add content-aware preflight in this pass only if it remains local and deterministic:

```rust
pub fn run_content_preflight_checks(
    targets: &[SecurityReviewTarget],
    load_content: impl Fn(&Path) -> Option<String>,
) -> Vec<SecurityPreflightResult>
```

Initial checks should be simple and defensive:

- hardcoded secret-like assignment in changed target file content;
- unsafe keyword in changed target content;
- process execution APIs in changed target content;
- SQL string construction with format/interpolation hints;
- weak crypto names (`md5`, `sha1`, `des`) in changed target content.

Important: treat these as evidence candidates, not automatic findings.

Acceptance criteria:

- evidence conversion is deterministic;
- preflight remains local; no network; no external scanner requirement;
- content checks are explicitly heuristic.

## Finding Synthesis Function

Add a new function; do not overload the current prompt-only `synthesize_findings` until behavior is stable.

```rust
pub fn synthesize_evidence_based_findings(
    targets: &[SecurityReviewTarget],
    prompts: &[SecurityReviewPrompt],
    preflight: &[SecurityPreflightResult],
) -> (Vec<SecurityReviewFinding>, Vec<SecurityReviewPrompt>)
```

Behavior:

1. Group prompts/evidence by file and nearby line.
2. Convert target, marker, and preflight evidence to structured evidence.
3. Apply eligibility gate.
4. Emit findings only for eligible groups.
5. Return remaining prompts for non-eligible groups.

Grouping recommendation:

```text
key = file_path + line bucket
line bucket = exact line if present, else file-level
nearby lines: +/- 5 lines can be grouped if same category
```

Acceptance criteria:

- marker-only prompts remain prompts;
- eligible prompt/evidence groups can become findings;
- ineligible prompts are preserved, not dropped.

## Severity and Confidence Rules

Implement deterministic mapping first. Keep conservative defaults.

Severity input factors:

```text
Critical: do not emit in this pass unless deterministic evidence is overwhelming; default avoid.
High: auth bypass/secret exposure/process execution reachable from public boundary with call path or strong reasoning.
Medium: unsafe/process/sql/secret risk in changed hunk with marker + supporting evidence.
Low: heuristic risk with changed hunk and plausible but incomplete support.
Info: review-only advisory or truncation/context limitation.
```

Confidence input factors:

```text
High: deterministic preflight + exact changed line + marker/category agreement.
Medium: marker + changed hunk + category-specific reasoning.
Low: marker + weak supporting context or truncated context.
```

Recommended helper:

```rust
pub fn classify_finding(
    category: Option<&str>,
    evidence: &[SecurityEvidence],
    truncated: bool,
) -> (SecuritySeverity, SecurityConfidence)
```

Rules:

- truncation should reduce confidence by one level unless evidence is deterministic;
- filename-only preflight should never raise above Low confidence on its own;
- no Critical by default.

Acceptance criteria:

- severity/confidence are deterministic;
- tests cover category/evidence combinations;
- confidence is reduced by truncation.

## Finding Text Generation

Add deterministic text helpers:

```rust
fn finding_title(category: Option<&str>, evidence: &[SecurityEvidence]) -> String
fn finding_reasoning(evidence: &[SecurityEvidence]) -> String
fn finding_recommendation(category: Option<&str>) -> String
fn finding_tests(category: Option<&str>) -> Vec<String>
```

Recommendations should be defensive and minimal:

- auth: add validation/negative tests, enforce issuer/audience/expiry checks;
- secrets: remove hardcoded secret, load from secret store/env, add secret-scan regression;
- unsafe: document invariant, add boundary checks, reduce unsafe scope;
- process: avoid shell interpolation, pass args separately, add malicious input test;
- filesystem/path: canonicalize and enforce root, add traversal tests;
- sql: use parameterized queries, add injection regression;
- crypto: use modern primitives, avoid weak hash/ECB/hardcoded key.

Acceptance criteria:

- recommendations are defensive;
- no exploit instructions;
- tests are concrete but not offensive payload recipes.

## Report Assembly Update

Add a new report assembly function for synthesis:

```rust
pub fn assemble_security_review_report_with_findings(
    targets: Vec<SecurityReviewTarget>,
    prompts: Vec<SecurityReviewPrompt>,
    findings: Vec<SecurityReviewFinding>,
    notes: Vec<String>,
) -> SecurityReviewOutput
```

Keep the existing vertical-slice `SecurityReviewReport` behavior unchanged for compatibility.

Notes should include:

```text
risk markers are review prompts unless supported by additional evidence
findings are heuristic defensive review outputs, not proof of exploitability
```

Acceptance criteria:

- old report API remains marker-only and stable;
- new synthesis report can carry findings;
- notes preserve conservative semantics.

## Tests

Add hermetic tests. No live LSP required.

### Type/eligibility tests

```text
security_finding_marker_only_not_eligible
security_finding_changed_hunk_only_not_eligible
security_finding_marker_plus_changed_hunk_eligible
security_finding_preflight_plus_changed_hunk_eligible
security_finding_truncation_lowers_confidence
```

### Evidence conversion tests

```text
security_evidence_from_changed_target
security_evidence_from_risk_marker_prompt
security_evidence_from_preflight_failure
security_evidence_preserves_file_and_line
```

### Synthesis tests

```text
security_synthesis_marker_only_remains_prompt
security_synthesis_marker_plus_changed_hunk_emits_finding
security_synthesis_preflight_filename_only_remains_prompt
security_synthesis_content_preflight_plus_changed_hunk_emits_finding
security_synthesis_ineligible_prompts_are_preserved
```

### Classification tests

```text
security_classify_auth_with_call_path_medium_or_high
security_classify_secret_with_content_preflight_medium
security_classify_filename_hint_low_confidence
security_classify_no_critical_by_default
```

### Text tests

```text
security_recommendation_auth_is_defensive
security_recommendation_process_avoids_shell_interpolation
security_recommendation_sql_mentions_parameterized_queries
security_tests_are_defensive_regression_tests
```

Acceptance criteria:

- marker-only invariant remains tested;
- evidence gate is tested;
- no network/LSP dependency in synthesis tests.

## Documentation

Update:

```text
AGENTS.md
architecture/tool.md
architecture/lsp.md
.opencode/skills/security/SKILL.md
```

Add a section:

```markdown
### Evidence-based security findings

The security workflow separates review prompts from findings. Risk markers remain prompts unless additional evidence supports a concrete issue. Finding synthesis is conservative: severity and confidence are deterministic, recommendations are defensive, and outputs are not proof of exploitability.
```

Document:

- marker-only invariant;
- evidence requirements;
- severity/confidence semantics;
- deterministic/local-only checks;
- no exploit/no mutation/no network boundary.

Acceptance criteria:

- docs do not imply risk markers are vulnerabilities;
- docs describe findings as defensive review outputs;
- docs explain confidence limitations.

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
cargo test -p codegg security_finding
cargo test -p codegg security_synthesis
cargo test -p codegg security_evidence
cargo test -p codegg security_classify
rg "SecuritySeverity|SecurityConfidence|SecurityEvidenceKind|synthesize_evidence_based_findings|is_finding_eligible" src/security tests architecture AGENTS.md .opencode
rg "risk markers are review prompts|not proof of exploitability|defensive" src/security tests architecture AGENTS.md .opencode
rg "exploit|payload|offensive|attack" src/security tests architecture AGENTS.md .opencode
```

Manual smoke:

```text
1. Marker-only context -> review prompt, zero findings.
2. Changed hunk + risk marker -> low/medium finding depending on category and evidence.
3. Filename-only secret hint -> prompt only, no finding.
4. Content preflight secret-like assignment + changed hunk -> finding with defensive recommendation.
5. Truncated context -> lower confidence or note.
```

## Done Criteria

This pass is complete when:

- active finding synthesis exists behind an explicit new function;
- severity and confidence are enums;
- structured evidence is used;
- marker-only evidence never creates findings;
- finding eligibility gate is central and tested;
- deterministic content preflight is local-only and heuristic;
- findings include severity, confidence, evidence, reasoning, recommendation, and tests;
- ineligible prompts are preserved;
- docs explain conservative semantics and safety boundaries;
- no mutation, exploit workflow, network scanning, or dependency/CVE lookup is introduced.

## Follow-Up Passes

After this lands:

1. Cleanup/hardening for finding synthesis.
2. Optional `securityContext` call expansion escalation for high-risk eligible groups.
3. TUI/CLI rendering of prompts vs findings.
4. Dependency metadata/CVE context for dependency review.
5. Project-level policy config for severity thresholds and ignored paths.
