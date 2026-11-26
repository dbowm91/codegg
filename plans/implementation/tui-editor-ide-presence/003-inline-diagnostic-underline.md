# TUI Editor IDE Presence Milestone 003 — Inline diagnostic underline

Status: closed

Closure record:

- `plans/closure/tui-editor-ide-presence/003-status.md`

Repository baseline: `d585e75f895cf41cd2241512d8d7e7c37fcece99` (re-inspect at M002's closure SHA before editing)

Source roadmap:

- `plans/subsystems/tui-editor-ide-presence-roadmap.md#milestone-3--inline-diagnostic-underline`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/001-terminology-and-domain-model.md#6-session-and-interaction-terms`

Applicable ADRs:

- `plans/adrs/ADR-0012-native-lsp-read-surface-delivery-and-authorization.md`
- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`

Primary class: capability

## 1. Objective

A Ready diagnostics pull draws a severity underline on the spanned text and keeps the existing gutter glyph. The user can see a diagnostic without opening `F2`.

## 2. Dependency readiness

The hard dependency is satisfied: M002 is closed (`plans/closure/tui-editor-ide-presence/002-status.md`), and its status cluster distinguishes warming from ready. The diagnostics pull and gutter glyph already exist; this milestone presents the Ready snapshot the editor already holds.

One validation attempt was recorded as blocked because an unrelated in-flight refactor in `src/core/daemon_documents.rs` had the tree uncompilable at that moment. That refactor belonged to M006 and landed afterwards; the blocker was transient, not an M001 contract defect. The milestone is closed against the verification in `plans/closure/tui-editor-ide-presence/003-status.md`.

## 3. Current implementation evidence

The editor gutter maps the minimum severity on a line to `!` (error), `▲` (warning), or `·` (other). Ready diagnostic ranges now produce severity underlines on the visible text. The line painter uses the existing bounded UTF-16 position conversion, clips to the rendered slice, and declines malformed positions, off-document paths, and over-budget lines. `refresh_editor_diagnostics` still replaces the retained set only on Ready; a missing or failed pull keeps the last complete display, so underlines and the gutter share the same snapshot semantics.

Component and render regression cases cover overlap and clipping, NotReady retention, Ready-empty clearing, path/generation fences, and unchanged row geometry. They run and pass; the commands and counts are in the closure record.

The `F2` panel lists diagnostics. `apply_editor_opened` does not open it. M002 keeps it manual. Semantic-token drawing already styles spans in the line. An underline is another span attribute, not a second buffer.

Diagnostics are pull-authoritative. This milestone does not subscribe to a new stream.

## 4. Invariants that must not regress

- Only a Ready pull replaces the drawn diagnostics, including underlines.
- `NotReady`, failure, and a generation mismatch do not clear the last Ready underlines and do not clear the gutter glyph.
- The gutter glyph stays. The underline does not replace it.
- `F2` is not opened to reveal an underline.
- No new diagnostics protocol, no code action, and no frontend language server.
- Underlines are clipped to the viewport row the renderer already budgets. They do not allocate a line the frame budget refused to read.
- Severity colors come from the theme tokens the editor already uses for error, warning, and muted. No hardcoded selection color. `scripts/check_no_hardcoded_selection_colours.py` stays green if a color is added.

## 5. Scope

### In scope

- Underline the Ready diagnostic ranges that intersect the visible line text.
- Keep the gutter glyph.
- A regression that `NotReady` does not clear underlines.

### Explicitly out of scope

- Auto-opening `F2`, hover text on the underline, and code actions.
- A push subscription or a change to `LspDiagnosticsGet`.
- Squiggles that require a font the terminal does not have. Use the style attribute the renderer already supports (underlined, or a theme foreground if underline is not available). Do not draw a second row of caret characters under the line. That would change row geometry.
- Desktop editor presentation.

## 6. Required production changes

### Core/domain

None.

### Storage and migrations

None.

### Protocol and DTOs

None. Ranges already arrive on the diagnostics pull.

### Runtime and concurrency

The underline set is the same snapshot as the gutter. One Ready replacement updates both. A stale generation drops both updates.

### Frontend or operator surface

When painting a visible line, apply an underline style to the columns the Ready diagnostics cover on that line. Error, warning, and other severities are distinguishable. Overlapping ranges use the higher severity (the same minimum-severity order the gutter uses: error before warning before other).

Ranges are UTF-16 on the wire, as the completion path already treats cursor positions. Convert through the existing bounded position helper. A range that falls outside the rendered slice is clipped, not a reason to read the rest of a giant line. A range the helper refuses is not drawn and does not fail the frame.

The gutter glyph is unchanged.

### Security and authorization

No new read. The underline is not a navigation command. Selecting an underlined span does not open a file.

### Documentation and static guards

`docs/tui.md` says diagnostics are visible in the text and listed with `F2`. `architecture/tui.md` says the underline is presentation of the Ready pull, not a second authority.

## 7. Ordered work packages

### Work package A — Span from a Ready snapshot

Intent: pure function from visible line, diagnostic ranges, and severity to styled spans.

Required changes: clip to the rendered slice. Higher severity wins on overlap. Refuse an over-budget line the same way the gutter already refuses to read it.

Acceptance evidence: a unit test with an error and a warning on one line, a range past the rendered slice, and an empty Ready list that clears underlines only because it is Ready.

### Work package B — Paint with the gutter

Intent: the frame draws the glyph and the underline from the same snapshot.

Required changes: the line painter calls the span function. No second geometry row.

Acceptance evidence: a render test shows the gutter marker and an underlined span, and the row count is unchanged.

### Work package C — NotReady preserves

Intent: a not-ready pull does not wipe underlines painted from the last Ready snapshot.

Required changes: the existing preserve-on-failure path covers the underline snapshot. Do not add a parallel cache.

Acceptance evidence: a Ready snapshot with one error, then a `NotReady` pull, still renders the underline and the glyph.

## 8. Failure, cancellation, restart, and contention semantics

Failure and cancellation of the pull keep the last Ready draw, as today. A new document generation clears diagnostics with the rest of the editor LSP state. A pull for a different path or generation is dropped.

An empty Ready list clears underlines and glyphs. That is a real empty result, and M002's status word is what says the server was ready. This milestone must not clear on any other outcome.

## 9. Compatibility and migration

No protocol or config change. Terminals that cannot underline use the theme foreground the renderer already has for that severity. The closure names which attribute was used. Both must keep error and warning distinguishable.

## 10. Required tests

### Focused unit tests

- Overlap, clip, empty Ready clear, and over-budget line refusal.

### Integration tests

- Rendered frame contains the glyph and the underline, and does not open `F2`.
- `NotReady` after Ready preserves both.

### Restart and recovery tests

None.

### Contention and cancellation tests

- A stale generation does not clear or replace the current underlines.

### Security and negative tests

- A diagnostic whose path is not the open document is not drawn. The existing scope check is the one to extend, not a new path join.

### Migration and compatibility tests

None.

## 11. Required verification commands

```bash
python3 scripts/check_tui_editor_text_authority.py
scripts/verify.sh scoped
```

Run the TUI render family that owns the editor component, selected from `docs/test-ownership.toml`.

## 12. Documentation updates

- `docs/tui.md`: in-text diagnostics and `F2` as the list.
- `architecture/tui.md`: underline is not an authority.

## 13. Acceptance criteria

- A Ready error and a Ready warning are distinguishable in the text, and the gutter glyph is still drawn.
- `F2` does not open because a diagnostic arrived.
- `NotReady` does not clear the last Ready underline.
- An empty Ready list does clear it.
- Row geometry is unchanged. No second caret row.
- The text-authority guard is green.

## 14. Stop conditions

- M002 has not closed.
- Distinguishing severity requires a second text row or a hardcoded color the selection-color guard rejects.
- The only range data is a push event that does not exist. Do not add one.
- UTF-16 conversion cannot be done with the existing bounded helper without reading a giant line. Report that line as refused, which is the gutter's rule. Do not materialize it.

## 15. Closure evidence required

- The span tests and the render test.
- The `NotReady` preservation test.
- The attribute used (underline or theme foreground), named.
- The text-authority guard and scoped verify, labeled local.
- No live TUI receipt required.

## 16. Handoff notes

This milestone does not unblock M004, M005, or M006. Those wait on M001. Do not start them here.

Do not edit the protected untracked scripts. Do not reopen ADR-0064 or the closed diagnostics-presentation milestone except by reference.
