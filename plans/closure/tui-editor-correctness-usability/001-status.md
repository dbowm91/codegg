# TUI Editor Correctness and Usability C001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-editor-correctness-usability/001-semantic-token-fidelity.md`

Source subsystem roadmap:

- `plans/subsystems/tui-editor-correctness-usability-roadmap.md#C001`

Repository baseline reviewed: `8291ee324640b85094b72d3cf1e52548c0ce9f57`

Implementation commits or pull requests:

- `32fd7505ee22a76cbd1b0395f6968dfe14cfc052` — tui-editor(c001): semantic token fidelity and bounded validation

## 1. Executive finding

C001 is complete as a corrective frontend slice. Ordinary valid semantic-token
responses with 101/500/1000 tokens and mixed standard Rust kinds
(`variable`, `parameter`, `property`, `namespace` plus keyword/string/number/
comment/function) now highlight instead of losing all styles to the old
100-item panel cap or a single unstyled kind. Structural validation, theme
style mapping, and bounded drawing are separated; malformed spans drop
individually; daemon truncation is preserved and disclosed; stale results are
discarded by generation/scope/revision; file bytes are untouched. No protocol,
storage, migration, or authorization change. Historical M006 closures are
preserved; this is new corrective evidence only.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Valid >100-token response paints known styles; no 100-item whole-discard | `counts_101_500_and_1000_are_accepted_but_over_cap_fails_closed` (101/500/1000 kept, 1001 fails closed); `MAX_EDITOR_SEMANTIC_TOKENS == MAX_LSP_SEMANTIC_TOKENS` | pass | Cap is 1000; panel cap stays 100 and distinct |
| Mixed standard kinds incl. variable/parameter/property/namespace kept | `mixed_standard_kinds_including_variable_parameter_property_namespace_are_kept` (10/10 kept, 0 dropped) | pass | Unknown style is not malformation |
| Unknown types render plain, neighbors kept | `semantic_overlay_drops_malformed_spans_individually_and_keeps_unknown_plain`; `unknown_tokens_render_plain_while_neighbors_keep_styles` (3/3 kept, themed>=2) | pass | Validator accepts; `token_style` returns None → base |
| Unicode-safe (split surrogate, out-of-range, overlap) | `utf16_ranges_require_valid_utf8_boundaries`; `supplementary_character_spans_are_unicode_safe`; overlap drops later only | pass | Emoji fixture with styled tokens each side of unstyled |
| Malformed line/offset/zero-length dropped individually | `malformed_line_offset_and_zero_length_are_dropped_individually` (1 kept, 4 dropped incl. overlap) | pass | Absolute offsets; no later shift |
| Truncated result preserved and disclosed, never complete | `empty_ready_is_valid_and_truncation_is_preserved_as_incomplete`; `truncated_and_partial_highlights_are_disclosed_in_the_banner` | pass | `semantic_truncated`/`semantic_incomplete` + status-row note |
| Ready zero tokens is valid empty, never NotReady | Same empty/truncation test (`revision=Some`, flags false, note None) | pass | NotReady/error clears to `revision=None` in `apply_editor_lsp_read` |
| Theme-legible mapping, no hardcoded colors | `token_styles_come_from_theme_slots_and_stay_legible` (14 kinds on dark+light, fg!=bg; keyword=primary, comment=muted, string=success) | pass | Patch preserves current-line bg |
| Current-line/diagnostic indicators + horizontal clipping preserved | `semantic_highlight_preserves_current_line_and_diagnostic_markers`; `semantic_tokens_do_not_break_horizontal_clipping`; pre-existing viewport tests | pass | Gutter `!`, bg patch, 5000-char line stays ≤ viewport |
| Stale generation and revision never paint | `a_local_edit_undo_and_redo_clear_stale_semantic_state`; `detach_clears_semantic_so_reopened_path_starts_fresh`; `on_projection_reconnect_clears_stale_semantic_highlight`; generation check in `apply_editor_lsp_read` | pass | Edit/undo/redo/reload/resync/save/reconnect clear; late completion discarded |
| Reopened same path starts fresh | `detach_clears_semantic_so_reopened_path_starts_fresh` | pass | Detach bumps generation and clears |
| File bytes unaffected | Text-authority guard; no `DocumentController` mutation in diff; trajectory suites green | pass | Validator borrows snapshot only |
| Docs updated | `architecture/tui.md` C001 subsection; `docs/tui.md` limits/truncation subsection | pass | Style/limit/truncation semantics |

Before/after positive fixture: pre-fix `validate_semantic_tokens` returned
`Vec::new()` for any count >100 or any `variable`/`parameter`/`property`/
`namespace`/`vendor_custom` kind (proved by the old
`semantic_overlay_rejects_unknown_invalid_and_overlapping_spans` test, which
asserted empty for those cases). Post-fix the same shapes validate as above
(101/500/1000 kept; mixed 10/10 kept; unknown kept for plain render; overlap
drops 1/2). The old test was replaced, not edited in place, and its prior
assertion is recorded here as the before evidence.

## 3. Production implementation evidence

- `src/tui/editor_lsp.rs`: kept `MAX_EDITOR_READ_ITEMS=100` for panels;
  added `MAX_EDITOR_SEMANTIC_TOKENS = MAX_LSP_SEMANTIC_TOKENS` (1000).
  Added `semantic_truncated`/`semantic_incomplete` to `EditorLspState` with
  `clear_semantic`, `store_semantic` (preserves daemon `truncated`, sets
  incomplete on truncated/dropped/rejected_all), and `semantic_coverage_note`.
  Rewrote `validate_semantic_tokens` to return `ValidatedSemanticTokens`
  (`accepted`/`dropped`/`rejected_all`): only over-cap rejects all;
  zero-length, oversized type/modifier bounds, bad line, oversized line,
  read failure, split-surrogate/out-of-range UTF-16, empty/inverted range,
  and overlap drop individually after sorting by `(line, character)`.
  Unknown type names are accepted. Borrows snapshot only; no write effect.
- `src/tui/components/editor.rs`: replaced hardcoded
  `known_token_style` colors with `token_style(&Theme, _)` mapping through
  theme slots (keyword/modifier→primary bold, string/regexp→success,
  number→warning, comment→muted italic, function/method/macro→secondary,
  type/class/enum/struct/interface/typeParameter/namespace→primary,
  variable/parameter/property/enumMember/event/operator/decorator/label→
  foreground, else None→plain base). `line_spans` skips unknown for plain
  while keeping neighbors; `base.patch(style)` preserves current-line bg.
  Banner appends `· highlighting truncated` / `· highlighting partial` from
  `semantic_coverage_note`.
- `src/tui/commands/editor.rs`: `apply_editor_lsp_read` distinguishes Ready
  semantic payload (validates, stores with truncation, revision-gated) from
  NotReady/error/wrong-operation (clears to no-data); stale revision clears;
  panel is closed for semantic. `submit_edit`, `editor_undo`, `editor_redo`
  clear semantic (+ diagnostics stale). `apply_editor_operation_finished`
  clears on Reload/Resync/Save. No render-path server calls.
- `src/tui/app/project_session.rs`: `on_projection_reconnect` clears semantic
  and marks diagnostics stale.
- `src/tui/app/mod.rs`: reconnect test for semantic clearing.
- Docs: `architecture/tui.md` C001 subsection; `docs/tui.md`
  limits/truncation subsection.

Ownership preserved: `DocumentController` owns text; LSP data never mutates
text; only exact session/project/workspace/attachment/generation/revision
decorates.

## 4. Verification executed

### Commands run

```bash
cargo test --lib tui::editor_lsp --locked
cargo test --lib tui::components::editor --locked
cargo test --test lsp_read_surface_trajectory --locked
cargo test --test document_client_trajectory --locked
cargo test --lib --locked tui::
python3 scripts/check_tui_editor_text_authority.py
python3 scripts/check_tui_project_authority.py
python3 scripts/check_authorization_matrix.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
scripts/verify.sh quick
```

All suites exist under the plan's names; no substitutes were needed.

### Results

- `tui::editor_lsp`: 11 passed / 0 failed (includes 5 new C001 tests).
- `tui::components::editor`: 23 passed / 0 failed (includes 5 new C001 tests).
- `tui::commands::editor`: 38 passed / 0 failed (includes 2 new stale tests).
- Broad `tui::`: 1448 passed / 0 failed.
- `lsp_read_surface_trajectory`: 14 passed / 0 failed (predecessor properties intact).
- `document_client_trajectory`: 3 passed / 0 failed.
- `check_tui_editor_text_authority.py`: pass (one borrowed-snapshot note).
- `check_tui_project_authority.py`: pass.
- `check_authorization_matrix.py`: pass.
- `cargo fmt --check`: pass.
- `cargo clippy --workspace --all-targets -- -D warnings`: pass.
- `scripts/verify.sh quick`: pass (fmt, agent schema, core/client/desktop
  boundaries, sandbox, execution-ownership, TUI authority/project authority,
  http-route-disposition, audit, scheduler-bypass, config-merge, provider
  wire/catalog/resilience, OpenAI endpoint, Eggwork routing, workspace check).

Local truth only. Hosted root CI and Desktop E2E were not run from this
environment (see §10 low finding). Relevant TUI renderer tests above ran
locally on the implementation head.

## 5. Invariant review

- Only `DocumentController` owns text; no frontend cache/second buffer:
  guard green; validator takes `&DocumentSnapshot`, returns owned DTOs.
- LSP data cannot mutate text: no mutation path in diff; `submit_edit` is the
  sole writer and clears semantic.
- Exact identity decorates: generation + `current_lsp_scope` + snapshot
  revision checks in `apply_editor_lsp_read`; `reset_for_scope`/`detach`
  invalidate.
- UTF-16→UTF-8 rejects split surrogates/out-of-range/overlap: per-token
  drops proven by emoji/overlap tests.
- Unknown style is not malformed: accepted, rendered plain.
- Truncated is not complete: flags + banner note; stored verbatim.
- Ready zero is not NotReady: `Some` revision with flags false vs `None`.
- Warm-only, `file.read`, `LspPreviewApply` ownership: untouched; no new
  protocol/auth descriptor.

## 6. Failure and recovery review

- Late completion after edit/resync/reconnect/switch: discarded by
  generation/scope/revision; state cleared, never painted.
- Malformed/unauthorized token: dropped per-token or whole-rejected on
  over-cap; cannot paint different document or panic (all conversions are
  `Option`-gated, lines bounded at 16 KiB).
- No auto-retried speculative response: reads are explicit F-key/open/save
  triggered; failures clear to no-data.
- Truncated: incomplete coverage communicated; no tokens invented.
- Duplicate delivery: request-id + generation + revision gates; second
  completion for same request is refused by `apply_read`.
- Restart/contention: reconnect clears; lifecycle operations clear;
  detach bumps generation.

## 7. Migration and compatibility review

No storage migration, DTO change, new authorization descriptor, server
startup, or config change. Additive frontend behavior only. Existing clients
and daemons interoperate. M006 historical closures immutable; this record is
new corrective evidence.

## 8. Security review

No path/document authority broadened. `check_tui_editor_text_authority.py`
green (no retained buffer, no filesystem reads). `check_tui_project_authority.py`
green. `check_authorization_matrix.py` green. No secrets logged; truncation
bounds (`token_type>64`, `modifiers>16`, line 16 KiB, tokens 1000) bound
DoS. No `LspPreviewApply` shortcut; completion untouched.

## 9. Documentation and operations

- `architecture/tui.md`: C001 fidelity subsection (cap separation,
  per-token vs whole rejection, truncation disclosure, Ready-zero rule,
  theme slots, clearing points).
- `docs/tui.md`: user-facing limits/truncation subsection.
- Operational signal: status-row `highlighting truncated` / `highlighting
  partial`; absence with empty highlight means true empty, not warming.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Hosted root CI and Desktop E2E not run on `32fd7505` | Strict closure prefers exact-SHA hosted green; local `verify.sh quick` + 1448 TUI tests + clippy green substitute locally | Run hosted CI on the merge head; record runs here by factual correction if they fail |
| low | No live PTY/interactive TUI smoke | Roadmap calls one desirable; absence recorded, not claimed | Optional live smoke on a future pass |

No critical/high/medium findings. No corrective pass required.

## 11. Roadmap disposition

Milestone C001 closed; C002 remains ready and independent; M003 remains
blocked on proposed ADR-0021; M004 remains blocked on M003 + C001 (C001 now
satisfied, M003 still pending, so M004 stays blocked). No scope expansion
into completion.

## 12. Registry updates

- `plans/implementation/tui-editor-correctness-usability/001-semantic-token-fidelity.md`:
  `ready for handoff` → `implemented; closed`.
- `plans/subsystems/tui-editor-correctness-usability-roadmap.md` C001 row:
  `ready` → `closed`; closure linked.
- `plans/registry.md`: C001 `ready` → `closed` with closure
  `plans/closure/tui-editor-correctness-usability/001-status.md` and
  implementation `32fd7505`; unblock audit recorded (M004 still blocked on
  M003; nothing unblocked).
