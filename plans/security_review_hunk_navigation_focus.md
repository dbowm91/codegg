# Security Review Hunk Navigation and Focus Accuracy Plan

## Purpose

Polish the hunk-aware security review panel by making hunk focus line selection reliable and adding richer navigation between panel items, hunk context, and read-only source preview. This is the final focused UX pass before pivoting back to broader LSP integration/development.

Current state:

- `SecurityReviewOutput.hunks` carries parsed diff hunk refs.
- `SecurityReviewPanelItem.hunk` attaches hunk context to findings/prompts by file path and new-side line range.
- `Dialog::SecurityReview` renders hunk context with added/removed/context styling.
- `Dialog::SourcePreview` is read-only and root-scoped.
- `Enter` opens source preview via `resolve_security_review_item_path`, with clipboard fallback.

This pass should improve precision and navigation, not change finding synthesis.

## Non-Goals

Do not change evidence/finding synthesis rules.

Do not make LSP enrichment default.

Do not mutate files.

Do not add auto-fix/patch application.

Do not add network scanning, exploit payloads, or offensive flows.

Do not require live LSP in tests.

Do not turn the TUI into a full diff editor.

## Phase 1 — Audit Hunk Focus Construction

Find the conversion path from `ChangedHunk`/`DiffLine` to `SecurityReviewHunkRef`/`SecurityReviewHunkLine`.

Search:

```bash
rg "SecurityReviewHunkRef|SecurityReviewHunkLine|is_focus|DiffLineKind|ChangedHunk" src/security src/tui tests -n
rg "hunks:" src/security/workflow -n
```

Verify:

- old/new line counters are incremented correctly;
- added lines have `old_line=None`, `new_line=Some(n)`;
- removed lines have `old_line=Some(n)`, `new_line=None`;
- context lines have both line numbers;
- `is_focus` is set for the selected finding/prompt line, not globally for every use of the hunk.

Important design point:

A single `SecurityReviewHunkRef` reused by multiple panel items cannot store item-specific focus correctly unless the focus is computed during item projection or rendering. If the same hunk backs two prompts on different lines, each item needs a different focused line.

Preferred model:

```rust
pub struct SecurityReviewPanelItem {
    // existing fields
    pub hunk: Option<SecurityReviewHunkRef>,
    pub hunk_focus_line: Option<u32>,
}
```

or:

```rust
pub struct SecurityReviewHunkRef {
    // no is_focus stored here
    pub lines: Vec<SecurityReviewHunkLine>,
}

pub struct SecurityReviewHunkLine {
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub kind: SecurityReviewHunkLineKind,
    pub text: String,
}
```

Then compute focus in the panel renderer as:

```rust
let is_focus = item.line.is_some_and(|line| hunk_line.new_line == Some(line));
```

Acceptance criteria:

- focus is item-specific;
- two items sharing one hunk can focus different lines;
- no cloned hunk has stale/wrong `is_focus` data;
- removed-line-only hunks degrade gracefully.

## Phase 2 — Normalize Hunk Line Range Semantics

The current mapping appears to match by new-side range. Make that behavior explicit and robust.

Add helper:

```rust
pub fn hunk_contains_new_line(hunk: &SecurityReviewHunkRef, line: u32) -> bool
```

Rules:

- prefer actual hunk line entries: any line with `new_line == Some(line)`;
- fallback to range: `new_start <= line < new_start + new_lines`;
- if neither is available, return false.

Use this helper instead of manually comparing `line >= start && line < end`.

Acceptance criteria:

- hunk matching follows actual parsed lines when available;
- off-by-one behavior is covered by tests;
- mapping is documented as new-side matching.

## Phase 3 — Improve Finding/Prompt to Hunk Matching

Current mapping by file path + line in range is good but can be refined.

Recommended match order:

1. same file + exact `new_line == item.line` among hunk lines;
2. same file + item line inside hunk new range;
3. same file + evidence line inside hunk new range for findings;
4. no hunk.

For findings, use evidence fallback:

```rust
finding.evidence.iter()
    .filter(|ev| ev.file_path.as_ref() == Some(&finding.file_path))
    .filter_map(|ev| ev.line)
```

Only use evidence fallback for matching hunk context. Do not let it affect finding eligibility or severity/confidence.

Acceptance criteria:

- findings with no direct line but positioned same-file evidence can still attach hunk context;
- prompt matching remains line-based only unless prompt evidence has structured lines already;
- no matching fallback changes finding synthesis.

## Phase 4 — Add Hunk Navigation Within Detail View

Add simple navigation behavior for hunk-rich detail panes.

Suggested state additions to `SecurityReviewDialog`:

```rust
pub detail_section: SecurityReviewDetailSection,

pub enum SecurityReviewDetailSection {
    Summary,
    Hunk,
}
```

Suggested keys:

```text
h     jump detail scroll to hunk section / toggle summary-hunk focus
H     copy current hunk text to clipboard
Enter open read-only source preview
```

If section state is too much, add only `h` to jump `detail_scroll` to the first hunk line in the current detail rendering.

Acceptance criteria:

- hunk-backed items can jump quickly to hunk context;
- non-hunk items ignore `h` with no panic;
- copying hunk text is read-only and bounded;
- source preview remains Enter.

## Phase 5 — Add Cross-Item Hunk Navigation

Add optional quick navigation among hunk-backed items.

Suggested keys:

```text
]h    next hunk-backed item
[h    previous hunk-backed item
```

If multi-key support is awkward, use simpler single keys:

```text
]     next hunk-backed item
[     previous hunk-backed item
```

Behavior:

- searches visible items for next/previous item where `item.hunk.is_some()`;
- wraps around;
- resets detail scroll;
- no-op if no hunk-backed items.

Acceptance criteria:

- navigation works under all filters;
- selected index clamps/wraps correctly;
- no panic on empty item list.

## Phase 6 — Improve Source Preview Entry Point

When source preview opens from a hunk-backed item, center around the item’s line and optionally display a small banner indicating the hunk match.

Possible enhancement:

```rust
pub struct SourcePreviewDialog {
    pub origin_label: Option<String>, // e.g. "Security Review Finding"
}
```

Keep this optional; primary goal is focus accuracy.

Acceptance criteria:

- source preview still opens read-only;
- target line is highlighted;
- missing line opens top-of-file or error gracefully;
- root escape protection remains in the security review dialog before preview open.

## Phase 7 — Tests

Add tests for focus and navigation.

Suggested tests:

```text
security_review_hunk_line_numbers_added_removed_context_are_correct
security_review_hunk_focus_is_item_specific
security_review_two_items_same_hunk_focus_different_lines
security_review_hunk_contains_new_line_uses_actual_lines
security_review_hunk_contains_new_line_falls_back_to_range
security_review_finding_evidence_line_can_attach_hunk
security_review_hunk_backed_navigation_wraps_next_previous
security_review_hunk_jump_sets_detail_scroll_to_hunk_section
security_review_copy_hunk_text_is_bounded
security_review_removed_only_hunk_has_no_new_side_focus
```

Panel tests:

```text
security_review_hunk_render_highlights_only_selected_item_line
security_review_hunk_render_does_not_highlight_other_item_line
security_review_hunk_key_noops_when_no_hunk
```

Acceptance criteria:

- tests do not require live LSP;
- tests do not mutate real repo files;
- temp fixtures are allowed;
- focus-line correctness is covered directly.

## Phase 8 — Docs Updates

Update:

```text
README.md
AGENTS.md
architecture/lsp.md
architecture/tool.md
.opencode/skills/security/SKILL.md
```

Document:

- hunk matching is new-side line based;
- removed-only hunks may not focus a line unless future old-side matching is added;
- `h`, `H`, `[`/`]` or chosen keys;
- source preview remains read-only/root-scoped;
- hunk context is review context, not proof of exploitability.

Acceptance criteria:

- docs match exact keybindings implemented;
- docs clarify focus limitations;
- docs preserve safety semantics.

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
rg "hunk_focus|hunk_contains_new_line|HunkBacked|SecurityReviewHunk" src/security src/tui tests README.md AGENTS.md architecture .opencode
```

Manual smoke:

```text
1. Create a diff with two changed lines in the same hunk.
2. Ensure two review prompts/finding-like items can focus different lines in the same hunk.
3. Open /security-review --changed --panel.
4. Use h to jump to hunk context.
5. Use next/previous hunk-backed navigation.
6. Press Enter and verify source preview highlights the selected item line.
7. Confirm no file mutation path exists.
```

## Done Criteria

This phase is complete when:

- hunk focus is item-specific and reliable;
- exact/new-side hunk matching is centralized in helper functions;
- evidence-line fallback improves finding-to-hunk attachment without changing synthesis;
- hunk context is quickly navigable in the panel;
- source preview remains read-only and root-scoped;
- tests cover line numbering, focus behavior, navigation, and fallback;
- docs match actual behavior.

## After This: Pivot to Broader LSP Work

After this plan lands, the security review panel should be mature enough to stop iterating on immediate UX plumbing. The next broader LSP phase should focus on shared LSP infrastructure: capability discovery, diagnostics/cache lifecycle, remote-core ownership, and semantic context APIs usable by more than security review.
