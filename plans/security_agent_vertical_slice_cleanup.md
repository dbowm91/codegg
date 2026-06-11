# Security Agent Vertical Slice Cleanup Plan

## Purpose

Tighten the first security-agent vertical slice so the workflow skeleton is internally consistent before adding evidence-based finding synthesis.

The vertical slice is mostly landed:

- `src/security/workflow.rs` exists and is exported through `src/security/mod.rs`.
- Changed hunk parsing exists.
- Exclusion rules exist.
- Deterministic preset selection exists.
- Target building exists.
- `securityContext` request payload construction exists.
- Risk markers become review prompts, not findings.
- Report assembly keeps findings empty and adds a marker-not-finding note.
- Tests are broad.

This cleanup should focus on consistency, naming, and small contract fixes. Do not add full finding synthesis yet.

## Current Issues

1. `discover_targets_from_diff` has behavior drift from the pure target builder:

- hunk targets are created with `column: None` even though they have a line;
- preset selection uses path only via `select_preset_for_file`, losing content-based `unsafe_review`, `web_backend`, and `rust_cli` classification;
- async discovery does not dedupe targets;
- async discovery does not reuse `build_security_review_targets`, so future behavior can diverge.

2. `run_preflight_checks` is safe but misleadingly named:

- it scans only target file names, not file contents;
- names like `secret_pattern_scan` and `unsafe_pattern_scan` sound stronger than they are.

3. The module includes future-facing `SecurityReviewFinding`, `SecurityReviewOutput`, `SecurityPreflightResult`, and `SecurityEvidence` scaffolding. That is acceptable, but docs/comments should make clear that full finding synthesis is not active.

4. `plan_security_review_from_diff` emits generic changed-code prompts, while `prompts_from_security_context` emits marker-driven prompts. This is acceptable, but the report should distinguish `planned target prompts` from `risk marker prompts` in wording/evidence.

## Non-Goals

Do not implement confirmed findings.

Do not add severity/confidence synthesis logic.

Do not add dependency/CVE lookup.

Do not add taint analysis.

Do not enable call expansion in the vertical slice.

Do not add mutation.

Do not add network behavior.

Do not add mandatory command/TUI integration.

## Phase 1 — Normalize Async Discovery Through Pure Target Builder

Refactor `discover_targets_from_diff` to reuse `parse_changed_hunks` and `build_security_review_targets` as much as possible.

Current issue pattern:

```rust
for hunk in &hunks {
    targets.push(SecurityReviewTarget {
        file_path: hunk.file_path.clone(),
        line: Some(hunk.new_start),
        column: None,
        preset: preset.clone(),
        reason: SecurityTargetReason::ChangedHunk,
    });
}
```

Desired behavior:

- positioned hunk targets must use `column: Some(1)`;
- async path should dedupe the same way as pure builder;
- async path should use content hints when practical.

Recommended approach:

```rust
let mut all_hunks = Vec::new();
let mut content_by_path = HashMap<PathBuf, String>;

for file in &summary.files {
    if deleted or skipped { continue; }
    let path = PathBuf::from(&file.path);
    let file_diff = egggit::file_diff(root, &path, base).await?;
    let hunks = parse_changed_hunks(&file_diff.patch);

    if let Ok(content) = std::fs::read_to_string(root.join(&path)) {
        content_by_path.insert(path.clone(), content);
    }

    if hunks.is_empty() {
        // Use a file-level synthetic target only when no hunks are parseable.
        // This should also go through a helper if possible.
    } else {
        all_hunks.extend(hunks);
    }
}

let mut targets = build_security_review_targets(&all_hunks, |path| {
    content_by_path.get(path).cloned()
});
```

For file-level targets with no hunks, add a helper:

```rust
pub fn build_file_level_security_review_target(
    path: &Path,
    content_hint: Option<&str>,
) -> Option<SecurityReviewTarget>
```

This helper should:

- skip excluded paths;
- select preset with content hint;
- set `line=None`, `column=None`;
- infer reason consistently.

Acceptance criteria:

- async discovery hunk targets use `column: Some(1)`;
- async discovery uses content hints when available;
- async discovery dedupes targets;
- pure and async paths share helper logic.

## Phase 2 — Fix File Path Handling in Parsed Per-File Diffs

`parse_changed_hunks` tracks paths from `diff --git a/... b/...`. If `egggit::file_diff` returns patches without full `diff --git` headers, hunks may not parse because `current_file` stays `None`.

Add a helper for per-file patches:

```rust
pub fn parse_changed_hunks_for_file(diff: &str, file_path: &Path) -> Vec<ChangedHunk>
```

Behavior:

- if `parse_changed_hunks(diff)` returns nonempty, use it;
- otherwise, parse hunk headers using `file_path` as the current file;
- still skip binary/deleted markers where visible.

Acceptance criteria:

- per-file patches without `diff --git` headers still produce hunks;
- existing multi-file parsing remains unchanged;
- tests cover both forms.

## Phase 3 — Rename or Clarify Filename-Only Preflight Checks

The current `run_preflight_checks` performs filename-only checks. Keep it if useful, but make naming explicit.

Options:

### Preferred

Rename public check names:

```text
secret_filename_hint_scan
unsafe_filename_hint_scan
```

And update notes:

```text
No secret filename hints detected in target file names
Unsafe-like filename hints found in target file names
```

### Alternative

Rename function to:

```rust
run_filename_preflight_checks
```

and keep `run_preflight_checks` as a wrapper for compatibility.

Do not claim content scanning.

Acceptance criteria:

- check names accurately say filename or path hint;
- notes accurately say filename/path only;
- tests updated accordingly.

## Phase 4 — Distinguish Planned Target Prompts from Risk Marker Prompts

`plan_security_review_from_diff` currently creates prompts like:

```text
Review changed code in <path>
```

That is useful as a planning prompt, not a risk-marker prompt.

Adjust prompt titles/evidence to make this explicit:

```text
Review changed hunk: <path>
```

Evidence should include:

```text
source: changed_hunk
preset: <preset>
reason: <reason>
no securityContext executed in this planning step
```

For `prompts_from_security_context`, evidence should include:

```text
source: securityContext.risk_marker
```

Acceptance criteria:

- diff-planning prompts and marker prompts are distinguishable;
- tests assert source evidence strings;
- findings remain empty.

## Phase 5 — Keep Future Finding Types Clearly Inert

Add or tighten comments on future-facing structs/functions:

```rust
/// Reserved for future evidence-based synthesis. This vertical slice does not emit this type.
pub struct SecurityReviewFinding { ... }
```

For `synthesize_findings`, consider renaming to avoid implying findings are produced:

### Preferred

```rust
pub fn synthesize_review_prompts(...)
    -> (Vec<SecurityReviewFinding>, Vec<SecurityReviewPrompt>)
```

or:

```rust
pub fn synthesize_marker_prompts(...)
    -> Vec<SecurityReviewPrompt>
```

If renaming would churn too much, keep the function but update the doc comment and tests to state it returns empty findings by design.

Acceptance criteria:

- no public docs/comments imply active confirmed finding synthesis;
- marker-only invariant remains explicit.

## Phase 6 — Tests

Add or update tests.

### Per-file diff parser

```text
security_review_parse_hunks_for_file_without_diff_git_header
security_review_parse_hunks_for_file_prefers_embedded_diff_path
security_review_parse_hunks_for_file_skips_deleted_or_binary
```

### Async discovery helpers

If `discover_targets_from_diff` is hard to test due to async egggit calls, test extracted helpers:

```text
security_review_file_level_target_uses_content_hint
security_review_file_level_target_skips_excluded_path
security_review_file_level_target_unpositioned
```

If a temp git repo fixture exists or is easy:

```text
discover_targets_from_diff_sets_column_for_hunk_targets
discover_targets_from_diff_uses_content_hint_for_preset
discover_targets_from_diff_dedupes_targets
```

### Preflight naming

```text
run_preflight_checks_uses_filename_hint_names
run_preflight_checks_notes_say_filename_only
```

### Prompt source evidence

```text
security_review_plan_prompt_has_changed_hunk_source
security_review_marker_prompt_has_security_context_marker_source
```

Acceptance criteria:

- the async/pure behavior drift is pinned;
- check naming is pinned;
- prompt source distinction is pinned.

## Phase 7 — Documentation Updates

Update:

```text
AGENTS.md
architecture/tool.md
architecture/lsp.md
.opencode/skills/security/SKILL.md if relevant
```

Add concise wording:

```markdown
The first security review workflow slice creates review targets and prompts. It does not emit confirmed security findings. Filename preflight checks are filename/path hints only, not content scans.
```

If `discover_targets_from_diff` remains async/egggit-backed, document:

```markdown
Async target discovery uses read-only egggit diff operations and does not mutate the worktree.
```

Acceptance criteria:

- docs match implemented behavior;
- no docs imply content scanning when only filenames are scanned;
- marker-vs-finding boundary remains explicit.

## Phase 8 — Validation Commands

Run:

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Targeted:

```bash
cargo test -p codegg security_review
rg "column: None|discover_targets_from_diff|parse_changed_hunks_for_file|filename_hint|risk markers are review prompts" src/security src tests architecture AGENTS.md .opencode
rg "secret_pattern_scan|unsafe_pattern_scan|secret_filename_hint|unsafe_filename_hint" src/security tests architecture AGENTS.md .opencode
```

Manual smoke:

```text
1. Feed a per-file patch without diff --git header; expect hunks.
2. Feed a diff for src/unsafe_block.rs; expect unsafe_review and column Some(1).
3. Feed a diff for src/auth/handler.rs; expect web_backend.
4. Feed filename api_key.rs; expect filename-hint preflight wording, not content-scan wording.
5. Feed marker-only context; expect review prompt and zero findings.
```

## Done Criteria

This cleanup is complete when:

- async discovery and pure target building produce consistent positioned targets;
- per-file patches without `diff --git` headers parse correctly;
- async discovery uses content hints when available or clearly documents why not;
- async discovery dedupes targets;
- filename-only preflight checks are named/documented honestly;
- planned target prompts and risk-marker prompts are distinguishable;
- future finding structs remain clearly inert in this slice;
- marker-only evidence still never creates findings;
- docs and tests reflect the final contract.

## Next Pass After This

Move to evidence-based finding synthesis:

- active `SecurityReviewFinding` output;
- severity/confidence enums instead of strings;
- finding requirements: concrete evidence, affected code, reasoning, recommendation, tests;
- deterministic preflight content checks;
- optional `securityContext` call expansion for high-risk targets;
- TUI/CLI presentation separating prompts from findings.
