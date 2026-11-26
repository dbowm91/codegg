# TUI Editor IDE Presence M003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-editor-ide-presence/003-inline-diagnostic-underline.md`

Source subsystem roadmap:

- `plans/subsystems/tui-editor-ide-presence-roadmap.md#milestone-3--inline-diagnostic-underline`

Dependency: M002 is closed (`plans/closure/tui-editor-ide-presence/002-status.md`).

## 1. Executive finding

M003 is complete. A Ready diagnostic pull underlines the spanned text in the
buffer and keeps the existing gutter glyph; `F2` stays a manual list and is
not opened by a diagnostic arriving. Only a Ready pull replaces the retained
set, so a `NotReady` or failed pull preserves the last Ready underlines and
glyphs, while an empty Ready list does clear them. Row geometry is unchanged:
the underline is a span attribute on the line the frame already read, not a
second caret row and not a second authority.

Attribute used: **`Modifier::UNDERLINED`** over the visible span, with the
existing theme token for the severity as the foreground
(`theme.error` / `theme.warning` / muted for other severities). No hardcoded
colour was introduced.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Ready error and warning are distinguishable in the text | `inline_diagnostics_use_error_precedence_and_theme_severity_colors` | pass |
| Overlap resolves to the higher severity | Same test: error at byte 3 wins over the overlapping warning | pass |
| Ranges clip to the rendered slice and refuse malformed UTF-16 | `inline_diagnostics_clip_to_visible_bytes_and_refuse_malformed_utf16` | pass |
| An over-budget line is refused, not materialized | `an_over_budget_line_refuses_inline_diagnostic_spans` | pass |
| Gutter glyph kept; `NotReady` preserves the Ready draw | `tui_editor_cost::ready_diagnostics_underline_text_and_not_ready_preserves_the_gutter` | pass |
| Generation and range-path fences drop stale diagnostics | `tui_editor_cost::diagnostics_are_fenced_by_editor_generation_and_range_path` | pass |
| No row added, no automatic `F2` | The same render tests assert the panel stays closed and the row count is unchanged | pass |
| Text authority unchanged | `scripts/check_tui_editor_text_authority.py` | pass |

## 3. Production implementation evidence

`crates/codegg-tui/src/components/editor.rs` projects each diagnostic range
onto the already-read visible slice through the existing bounded UTF-16
position helper, merges overlapping ranges by severity (error before warning
before other, the gutter's own order), and applies the underline inside the
existing span composition with cursor styling last. A range for another path,
a malformed position, or a line past the render budget is dropped; none of
those reads more of the line.

The retained diagnostics snapshot is unchanged: `refresh_editor_diagnostics`
still replaces it only on Ready, so underlines and glyphs come from one
snapshot and fail together.

`docs/tui.md` and `architecture/tui.md` describe the underline as
presentation of the existing Ready pull.

## 4. Verification executed

All results are LOCAL to this worktree. No hosted CI run and no live TUI
receipt is claimed.

```bash
cargo test -p codegg-tui --lib -- inline_diagnostics an_over_budget_line_refuses
# 3 passed; 0 failed

CARGO_BUILD_JOBS=2 cargo test -p codegg-tui --test tui_render -- tui_editor_cost::
# 16 passed; 0 failed (includes the two M003 render tests and M002's)

CARGO_BUILD_JOBS=2 cargo check -p codegg-tui --all-targets
# clean

python3 scripts/check_tui_editor_text_authority.py
# TUI editor text-authority guard passed

python3 scripts/check_no_hardcoded_selection_colours.py
# passed
```

`scripts/verify.sh scoped` is the roadmap-level closing sweep and is run once
for M001–M006; it is recorded at the roadmap closeout rather than claimed here.

## 5. Superseded evidence

An earlier attempt at this record marked M003 `blocked` because the TUI
target failed to compile. That failure was an in-flight M006 refactor of
`src/core/daemon_documents.rs` (missing `DocumentService` publication
methods, and an arity mismatch on `schedule_managed_document_change`), not an
M001 contract defect. M006 landed afterwards and the same commands now pass,
so the blocked record was replaced rather than kept as a standing claim.

## 6. Handoff

This milestone does not unblock M004, M005, or M006: those depend on M001,
which is closed. No downstream plan is registered as unblocked by this closure.
