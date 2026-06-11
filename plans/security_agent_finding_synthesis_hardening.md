# Security Agent Finding Synthesis Hardening Plan

## Purpose

Tighten the evidence-based finding synthesis pass before building UI/CLI presentation or deeper `securityContext` escalation.

The first synthesis pass landed the major pieces:

- `SecuritySeverity` and `SecurityConfidence` enums;
- `SecurityEvidenceKind` and structured evidence;
- active `SecurityReviewFinding` output;
- eligibility gate;
- deterministic content preflight;
- synthesis function preserving ineligible prompts;
- conservative classification and defensive recommendations.

The main correctness issue is evidence scoping: preflight evidence is currently represented as strings and converted to `StructuredSecurityEvidence` with `file_path=None`, which allows one file's preflight failure to support findings in unrelated prompt groups. This pass should fix that tightly.

## Non-Goals

Do not add TUI/CLI rendering.

Do not add dependency/CVE lookup.

Do not add network scans.

Do not add exploit or offensive workflow generation.

Do not enable call expansion by default.

Do not expand severity policy beyond conservative deterministic rules.

Do not mutate files.

## Phase 1 — Add Structured Preflight Evidence

Introduce a structured preflight evidence type instead of relying only on `Vec<String>`.

Recommended additions:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityPreflightEvidence {
    pub file_path: PathBuf,
    pub line: Option<u32>,
    pub summary: String,
    pub detail: Option<String>,
}
```

Update `SecurityPreflightResult` to include structured evidence:

```rust
pub struct SecurityPreflightResult {
    pub check_name: String,
    pub status: PreflightStatus,
    pub evidence: Vec<String>, // keep for compatibility
    pub structured_evidence: Vec<SecurityPreflightEvidence>,
    pub notes: Vec<String>,
}
```

If adding a field breaks too many tests, create a parallel type first:

```rust
pub struct StructuredSecurityPreflightResult { ... }
```

but prefer extending the existing type while keeping `evidence` for compatibility.

Acceptance criteria:

- every content preflight failure carries file path;
- line is included when cheaply known;
- legacy evidence strings remain for display/backward compatibility.

## Phase 2 — Populate Structured Evidence in Content Preflight

Update `run_content_preflight_checks`.

For each target/content pair:

- iterate with `enumerate()` to capture line numbers;
- when a pattern matches, emit `SecurityPreflightEvidence` with:
  - `file_path: target.file_path.clone()`;
  - `line: Some(line_index + 1)`;
  - `summary` matching the current string evidence;
  - `detail` containing check-specific context.

Example:

```rust
for (idx, line) in content.lines().enumerate() {
    if secret_like_assignment(line) {
        structured.push(SecurityPreflightEvidence {
            file_path: t.file_path.clone(),
            line: Some((idx + 1) as u32),
            summary: "hardcoded secret-like assignment in content".to_string(),
            detail: Some("local heuristic content scan".to_string()),
        });
    }
}
```

Keep content snippets out of structured evidence unless sanitized/truncated. Do not store full secret-like line values.

Acceptance criteria:

- `secret_content_scan`, `unsafe_content_scan`, `process_exec_scan`, `sql_injection_scan`, and `weak_crypto_scan` all emit structured file-scoped evidence;
- no full secret values are stored in evidence;
- legacy `evidence` strings still summarize findings.

## Phase 3 — Populate Structured Evidence in Filename Preflight

Update `run_preflight_checks` as well.

Filename-only hints should emit structured evidence with:

- file path;
- line `None`;
- detail explicitly saying `filename/path hint only`.

These should remain low-confidence hints and should not produce findings without changed-hunk evidence.

Acceptance criteria:

- filename preflight evidence is file-scoped;
- notes continue to say filename hints only;
- filename-only evidence alone remains ineligible.

## Phase 4 — Convert Preflight Evidence Without Global Bleed

Replace current synthesis conversion:

```rust
file_path: None,
line: None,
summary: format!("{}: {}", p.check_name, e),
```

with structured conversion:

```rust
fn structured_evidence_from_preflight(result: &SecurityPreflightResult)
    -> Vec<StructuredSecurityEvidence>
```

Mapping:

```rust
StructuredSecurityEvidence {
    kind: SecurityEvidenceKind::Preflight,
    file_path: Some(preflight_evidence.file_path.clone()),
    line: preflight_evidence.line,
    summary: format!("{}: {}", result.check_name, preflight_evidence.summary),
    detail: preflight_evidence.detail.clone().or_else(|| Some(result.notes.join("; "))),
}
```

Fallback only for legacy strings:

- If `structured_evidence` is empty but `evidence` strings exist, either:
  - keep them as prompt-only evidence and do not use for finding eligibility; or
  - parse a file path only if deterministic and tested.

Preferred: legacy string-only preflight should not be finding-eligible.

Acceptance criteria:

- preflight evidence used by finding synthesis always has file path;
- string-only legacy evidence cannot globally support every group;
- tests cover no cross-file bleed.

## Phase 5 — Tighten Group Join Semantics

Update `synthesize_evidence_based_findings` group join.

Current pattern allows global evidence when `pe.file_path.is_none()`. Remove this.

Desired behavior:

```rust
for pe in &preflight_evidence {
    if pe.file_path.as_deref() == Some(&key.file_path) {
        if evidence_line_matches_group(pe.line, key.line_bucket) {
            group_evidence.push(pe.clone());
        }
    }
}
```

Line matching rule:

- if group has line bucket and evidence has line, match only if evidence line is within same bucket or within +/-5 lines;
- if group has line bucket and evidence has no line, allow same-file file-level evidence;
- if group has no line bucket, allow same-file evidence;
- never allow different-file evidence.

Helper:

```rust
fn evidence_matches_group(
    evidence: &StructuredSecurityEvidence,
    file_path: &Path,
    line_bucket: Option<u32>,
) -> bool
```

Acceptance criteria:

- different-file evidence never supports a finding group;
- same-file evidence joins predictably;
- line-bucket behavior is tested.

## Phase 6 — Restrict Content Preflight to Changed-Hunk Locality Where Possible

The current content preflight scans entire files. This is acceptable for prompts, but risky for findings because it can elevate pre-existing unrelated issues.

Add optional locality-aware content preflight:

```rust
pub fn run_content_preflight_checks_for_targets(
    targets: &[SecurityReviewTarget],
    load_content: impl Fn(&Path) -> Option<String>,
) -> Vec<SecurityPreflightResult>
```

Behavior:

- for positioned targets, scan only a small line window around target line, e.g. +/-10 lines;
- for unpositioned targets, scan full file but mark evidence as file-level;
- line numbers must be preserved.

You may keep existing `run_content_preflight_checks` as a wrapper, but ensure synthesis prefers the locality-aware function in future call sites.

Acceptance criteria:

- positioned changed-hunk targets do not scan unrelated full-file content for finding eligibility;
- file-level targets remain supported;
- tests cover hunk-local vs unrelated distant content.

## Phase 7 — Rename Legacy Prompt-Only Synthesis

The old `synthesize_findings` function now conflicts semantically with active `synthesize_evidence_based_findings`.

Preferred approach:

```rust
pub fn synthesize_review_prompts_only(...) -> (Vec<SecurityReviewFinding>, Vec<SecurityReviewPrompt>)
```

Keep a deprecated wrapper:

```rust
#[deprecated(note = "use synthesize_review_prompts_only or synthesize_evidence_based_findings")]
pub fn synthesize_findings(...) -> ... {
    synthesize_review_prompts_only(...)
}
```

If deprecation warnings break tests/build, skip the attribute and use documentation only.

Acceptance criteria:

- prompt-only behavior has a clear name;
- active finding synthesis remains distinct;
- tests use the clearer name where feasible.

## Phase 8 — Harden Tests

Add tests focused on correctness and false-positive control.

Required tests:

```text
security_preflight_structured_evidence_has_file_path
security_preflight_structured_evidence_has_line_for_content_match
security_synthesis_preflight_different_file_does_not_support_finding
security_synthesis_preflight_same_file_supports_finding
security_synthesis_preflight_same_file_distant_line_does_not_support_positioned_group
security_synthesis_preflight_same_file_nearby_line_supports_positioned_group
security_synthesis_legacy_string_preflight_does_not_globally_support_group
security_content_preflight_hunk_local_ignores_distant_secret
security_content_preflight_hunk_local_detects_nearby_secret
security_prompt_only_synthesis_name_preserves_marker_only_behavior
```

Also keep existing tests:

```text
security_synthesis_marker_only_remains_prompt
security_synthesis_marker_plus_changed_hunk_emits_finding
security_synthesis_content_preflight_plus_changed_hunk_emits_finding
security_synthesis_ineligible_prompts_are_preserved
```

Acceptance criteria:

- cross-file bleed is impossible and tested;
- distant-line false positives are controlled;
- compatibility behavior remains tested.

## Phase 9 — Documentation Updates

Update docs where the security workflow is described:

```text
AGENTS.md
architecture/tool.md
architecture/lsp.md
.opencode/skills/security/SKILL.md
```

Add wording:

```markdown
Evidence-based findings only combine evidence from the same file and nearby changed-hunk context. Filename-only hints remain prompts unless supported by additional same-file evidence. Content preflight is local and deterministic; it is heuristic and not proof of exploitability.
```

Acceptance criteria:

- docs state same-file evidence scoping;
- docs state filename hints are weak evidence;
- docs preserve marker-not-finding and no-exploit semantics.

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
cargo test -p codegg security_preflight_structured
cargo test -p codegg security_synthesis_preflight
cargo test -p codegg security_content_preflight_hunk_local
cargo test -p codegg security_prompt_only
rg "structured_evidence|SecurityPreflightEvidence|evidence_matches_group|synthesize_review_prompts_only" src/security tests architecture AGENTS.md .opencode
rg "file_path: None|pe.file_path.is_none|synthesize_findings" src/security/workflow.rs
```

Manual smoke:

```text
1. File A has marker; File B has content preflight failure. Expect no finding for File A from File B evidence.
2. Same file, marker and nearby content preflight failure. Expect finding.
3. Same file, marker at line 10 and content failure at line 500. Expect prompt only for positioned review.
4. Filename-only secret hint without changed hunk. Expect prompt only.
5. Marker-only context. Expect prompt only.
```

## Done Criteria

This hardening pass is complete when:

- preflight evidence is structured and file-scoped;
- content preflight evidence includes line numbers when possible;
- synthesis never uses different-file evidence to support a finding;
- positioned synthesis respects nearby-line grouping;
- legacy string-only preflight evidence cannot globally support findings;
- content preflight can operate hunk-locally;
- prompt-only synthesis is clearly named;
- marker-only evidence still never creates findings;
- docs explain evidence scoping and heuristic limits.

## Next Pass After This

After hardening, move to one of:

1. `securityContext` call-expansion escalation for high-risk eligible groups.
2. TUI/CLI rendering of prompts vs findings.
3. Dependency metadata/CVE context for `dependency_review`.
4. Project policy configuration for severity thresholds and ignored paths.
