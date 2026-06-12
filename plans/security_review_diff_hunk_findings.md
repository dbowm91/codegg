# Security Review Diff/Hunk-Aware Findings Plan

## Purpose

Extend the security review result panel so each finding and review prompt can be grounded in the exact diff hunk that triggered it. The current panel can navigate to a read-only source preview at a resolved file/line; this pass should add hunk-aware context, changed-line highlighting, and hunk navigation without changing the security finding synthesis model.

This is a UX and evidence-localization pass. It should make findings easier to inspect during code review by showing:

- the changed hunk that produced the target;
- whether the finding line is added, removed, or unchanged context;
- nearby hunk lines with diff markers;
- links from panel items to their hunk view;
- optional source preview as a secondary action.

## Current State

Relevant current behavior:

- `/security-review` runs asynchronously and stores a structured `SecurityReviewReceipt`.
- `Dialog::SecurityReview` renders findings, prompts, notes, and preflight results.
- `Enter` on a file-backed panel item opens `Dialog::SourcePreview` through a root-scoped path resolver.
- `--panel` auto-opens the result panel on completion; default behavior remains non-disruptive.
- Receipts are in-memory only.
- The workflow already discovers targets from git diff hunks and stores target/hunk line information internally.

## Non-Goals

Do not change the evidence/finding synthesis rules.

Do not add dependency/CVE lookup.

Do not mutate source files.

Do not generate patches or auto-fixes.

Do not add exploit generation, network scanning, or offensive guidance.

Do not require live LSP in tests.

Do not implement a full git client UI. This is a focused hunk viewer for security review results.

## Phase 1 — Inventory Existing Diff/Hunk Structures

Inspect the current diff discovery and review target types.

Search:

```bash
rg "struct .*Hunk|DiffHunk|SecurityReviewTarget|ReviewTarget|hunk|added_start|old_start|new_start|line_start|discover_targets_from_diff" src/security src/tool crates tests -n
rg "diff_summary|diff_text|egggit|UnifiedDiff|@@" src crates tests -n
```

Identify:

- where diff hunk text is parsed;
- whether hunk line ranges are preserved;
- whether changed-line kind is preserved (`+`, `-`, context);
- how `SecurityReviewTarget` maps to findings/prompts;
- whether rendered findings preserve target ids or only file/line/title.

Deliverable:

- short code comments or docs note describing the current hunk data path;
- a minimal list of structs that should be extended.

## Phase 2 — Add Hunk DTOs to Security Review Output/Receipt Projection

Add a compact DTO for hunk context. Prefer a TUI-facing projection under `src/security/workflow/receipt.rs` unless workflow output already has a good place for it.

Suggested types:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityReviewHunkRef {
    pub file_path: PathBuf,
    pub old_start: Option<u32>,
    pub old_lines: Option<u32>,
    pub new_start: Option<u32>,
    pub new_lines: Option<u32>,
    pub lines: Vec<SecurityReviewHunkLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityReviewHunkLine {
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub kind: SecurityReviewHunkLineKind,
    pub text: String,
    pub is_focus: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SecurityReviewHunkLineKind {
    Added,
    Removed,
    Context,
}
```

Attach optional hunk context to panel items:

```rust
pub struct SecurityReviewPanelItem {
    // existing fields...
    pub hunk: Option<SecurityReviewHunkRef>,
}
```

If adding this field to the existing struct is too invasive, introduce a parallel lookup map:

```rust
pub struct SecurityReviewReceiptView {
    pub items: Vec<SecurityReviewPanelItem>,
    pub hunks_by_item: HashMap<usize, SecurityReviewHunkRef>,
}
```

Acceptance criteria:

- panel items can carry exact hunk context where available;
- missing hunk context is allowed and falls back to source preview;
- DTOs are serializable with the receipt;
- no full-file content is stored in receipt, only bounded hunk lines.

## Phase 3 — Preserve Target-to-Hunk Mapping During Review Assembly

Findings and prompts need to retain a link to the diff hunk that generated their target.

Recommended approach:

- add a stable `target_id` or hunk fingerprint to `SecurityReviewTarget`;
- carry it into review prompts;
- when findings are synthesized from prompts/evidence, preserve the best matching target/hunk by file path + line + prompt category/title;
- avoid brittle string-only matching where possible.

Suggested hunk fingerprint:

```rust
pub struct SecurityReviewHunkId {
    pub file_path: PathBuf,
    pub new_start: Option<u32>,
    pub old_start: Option<u32>,
    pub header: String,
}
```

Use deterministic mapping:

1. Exact file path + finding line inside hunk new range.
2. Exact file path + prompt line inside hunk new range.
3. Exact file path + title/category source target match.
4. Otherwise no hunk.

Acceptance criteria:

- findings/prompts generated from a diff hunk usually show that hunk;
- non-diff or file-level findings still render without hunk;
- matching is deterministic and testable;
- hunk mapping does not promote review prompts into confirmed findings.

## Phase 4 — Add Hunk View to Security Review Panel Detail

Enhance `SecurityReviewDialog` detail view.

For selected item with hunk context, render:

```text
Hunk: src/foo.rs @@ -10,6 +10,9 @@
  10  10  context line
      11 + added line
  12      - removed line
  13  12  context line
```

Display rules:

- `+` added lines use a distinct style;
- `-` removed lines use a distinct style;
- context lines muted/normal;
- focus line highlighted when it matches the finding/prompt line;
- hunk header visible;
- line numbers shown when available;
- if terminal is narrow, keep content readable rather than panicking.

Keep source preview as a separate action.

Suggested keybindings:

```text
Enter       open read-only source preview for selected item
h           toggle hunk/detail focus or jump to hunk section
H           copy hunk text to clipboard
```

If adding `h/H` is too much, simply show hunk in detail and keep Enter for source preview.

Acceptance criteria:

- hunk appears in the detail view when available;
- findings/prompts without hunk still render cleanly;
- no raw huge diff blobs are rendered unbounded;
- hunk text is read-only.

## Phase 5 — Add Optional Hunk-Only Filter/Toggle

Add a filter or toggle to focus hunk-backed items.

Suggested enum extension:

```rust
pub enum SecurityReviewFilter {
    All,
    Findings,
    Prompts,
    Notes,
    HighConfidence,
    MediumOrHigherSeverity,
    HunkBacked,
}
```

Or add a simple toggle:

```rust
pub show_only_hunk_backed: bool,
```

Acceptance criteria:

- user can quickly find results tied to changed hunks;
- filter does not hide notes permanently; it is reversible;
- selection clamps correctly after filter changes.

## Phase 6 — Add Diff/Hunk Navigation State If Needed

If detail view becomes crowded, split the panel into tabs:

```rust
pub enum SecurityReviewDetailTab {
    Summary,
    Evidence,
    Hunk,
    Source,
}
```

Recommended for this pass: avoid tabs unless necessary. Keep the detail panel simple: summary/evidence first, hunk below.

Acceptance criteria:

- minimal state added;
- no modal nesting complexity;
- source preview remains a separate dialog.

## Phase 7 — Tests

Add unit tests around hunk mapping and projection.

Suggested tests:

```text
security_review_hunk_ref_serializes_roundtrip
security_review_panel_item_includes_hunk_for_prompt_line_inside_hunk
security_review_panel_item_includes_hunk_for_finding_line_inside_hunk
security_review_panel_item_without_matching_hunk_has_none
security_review_hunk_filter_selects_only_hunk_backed_items
security_review_hunk_filter_clamps_selection
security_review_hunk_render_marks_added_removed_context_lines
security_review_hunk_focus_line_highlighted
security_review_hunk_context_is_bounded
security_review_hunk_mapping_does_not_promote_prompt_to_finding
```

Add integration-ish tests using fixture diff text if current diff parser supports it:

```text
security_review_receipt_projects_diff_hunk_context_from_fixture_diff
security_review_changed_file_deleted_has_no_hunk_preview
security_review_renamed_file_hunk_path_is_stable_or_gracefully_missing
```

Acceptance criteria:

- no live git repo required unless existing tests already use temp git repos;
- no live LSP required;
- no source mutation except temp fixture setup;
- edge cases covered for missing/renamed/deleted files.

## Phase 8 — Docs Updates

Update:

```text
README.md
AGENTS.md
architecture/lsp.md
architecture/tool.md
.opencode/skills/security/SKILL.md
.opencode/skills/agent-loop/SKILL.md
```

Document:

- result panel now shows hunk context when available;
- hunk context is derived from the reviewed diff;
- hunk-backed results are still defensive review aids, not proof of exploitability;
- prompts remain prompts, not confirmed findings;
- source preview remains read-only and root-scoped;
- no source mutations are possible from the panel.

Acceptance criteria:

- docs describe exact keybindings/toggles implemented;
- docs do not imply patch application or autofix behavior;
- docs explain fallback when no hunk is available.

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
cargo test -p codegg security_review_hunk
cargo test -p codegg security_review_panel
cargo test -p codegg security_review_receipt
rg "SecurityReviewHunk|HunkBacked|hunk" src/security src/tui tests README.md AGENTS.md architecture .opencode
```

Manual smoke:

```text
1. Modify a file with a small security-relevant change.
2. Run /security-review --changed --panel.
3. Confirm finding/prompt detail shows the relevant diff hunk.
4. Confirm added/removed/context lines are visually distinct.
5. Press Enter and confirm source preview remains read-only and root-scoped.
6. Test a finding without hunk context and confirm graceful fallback.
7. Cancel an in-flight review and confirm no stale hunk panel opens.
```

## Done Criteria

This phase is complete when:

- findings/prompts can carry bounded diff hunk context where available;
- the result panel renders hunk context clearly;
- source preview remains read-only and root-scoped;
- missing hunk context gracefully falls back to existing detail/source preview behavior;
- prompt/finding semantics remain unchanged;
- tests cover mapping, rendering projection, filters, and fallback;
- docs match actual behavior.

## Roadmap Placement

This pass is the next review-ergonomics layer after the result panel. It should be completed before larger LSP expansion because it makes current deterministic and LSP-enriched review output more inspectable and trustworthy.
