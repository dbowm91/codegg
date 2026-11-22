# TUI Editor IDE Presence M005 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-editor-ide-presence/005-viewport-semantic-tokens-and-off-thread-syntect.md`

Source subsystem roadmap:

- `plans/subsystems/tui-editor-ide-presence-roadmap.md#milestone-5--viewport-semantic-tokens-and-off-thread-syntect`

Dependency: M001 landed (operational only).

## 1. Executive finding

M005 is complete. The editor's semantic highlight asks for a bounded
`textDocument/semanticTokens/range` window from the viewport top instead of a
full-document dump the 1000-token cap would cut; a server that does not
advertise range support is refused rather than silently given the full read;
and the syntect default grammar/theme sets now load on their own thread, so no
frame can block on them. Syntect remains the floor for lines the server did not
colour and for a machine with no server at all.

`MAX_LSP_SEMANTIC_TOKENS` is **still 1000**. The closed draw-path milestone
(`plans/implementation/tui-render-cost/005-editor-draw-path-cost.md`) was not
reopened: this changes what the read asks for and which thread loads the
grammar, not the per-frame budget.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| The viewport request is a bounded range on the existing pull | `a_semantic_token_request_covers_a_bounded_viewport_window`; `semantic_tokens_range_uses_range_operation_when_advertised` | pass |
| A response still respects `MAX_LSP_SEMANTIC_TOKENS` | `semantic_token_cap_remains_one_thousand`; `semantic_token_payload_discards_overflow_and_sets_truncation_caveat` | pass |
| A missing range stays the legacy full read for other callers | `a_missing_semantic_token_range_round_trips_as_the_legacy_full_read` | pass |
| A range is only valid on the semantic-token operation and is non-empty | `a_semantic_token_range_is_only_valid_for_semantic_tokens_and_nonempty` | pass |
| No full read is sent for an over-cap document when range is unsupported | `unsupported_range_does_not_fall_back_to_a_full_read_for_an_over_cap_document` | pass |
| The render path never loads the default sets | `the_default_sets_are_loaded_only_by_the_loader` | pass |
| Syntect still paints the floor | 18 `editor_highlight` tests; 16 `tui_editor_cost` tests | pass |
| No tree-sitter dependency | no manifest change; `cargo check -p codegg-tui --all-targets` clean | pass |

## 3. Production implementation evidence

Protocol (`crates/codegg-protocol/src/lsp.rs`):
`LspSemanticTokenRangeDto { start_line, end_line }` (zero-based,
end-exclusive) as an optional `#[serde(default)]` field on
`LspReadRequestDto`; `validate()` refuses a range on any other operation and
an empty one, and refuses a workspace read that carries one.
`LspReadPayloadDto::semantic_tokens` applies the fixed cap and sets the
existing truncation caveat.

egglsp: the semantic-token operation now takes the range, checks the warm
server's advertised range capability (`supports_semantic_tokens_range`), and
issues `textDocument/semanticTokens/range`; without the capability the
request is refused rather than downgraded to the full read. The full read
remains the path for a request with no range.

TUI: `request_editor_semantic_tokens` sends a window of
`SEMANTIC_TOKEN_VIEWPORT_LINES` (160) from
`presentation().viewport_line`; a viewport change arms the existing 600 ms
quiet period, so a scroll burst is one read. `editor_highlight.rs` replaced
the `Lazy` statics with an `OnceCell<Arc<HighlightAssets>>` plus
`ensure_highlight_assets` (own OS thread, started at editor open and kicked
by the first span request). Every draw-path reader goes through `assets()`,
which can only observe the load. `wait_for_highlight_assets` is
`#[doc(hidden)]` test support: an integration test links the library without
the `cfg(test)` loader, and a frame must never wait.

Docs: `architecture/lsp.md` (range versus full read, cap unchanged) and
`architecture/tui.md` (the load's thread, the floor, the viewport window).

## 4. Verification executed

All results are LOCAL to this worktree. No hosted CI run and no live TUI
receipt is claimed.

```bash
CARGO_BUILD_JOBS=2 cargo test -p codegg-protocol --lib -- semantic
# 5 passed; 0 failed

CARGO_BUILD_JOBS=2 cargo test -p egglsp --features lsp-test-support --test code_action_preview
# 4 passed; 0 failed (both range cases)

CARGO_BUILD_JOBS=2 cargo test -p egglsp --lib -- semantic
# 36 passed; 0 failed

CARGO_BUILD_JOBS=2 cargo test -p codegg-tui --lib -- editor_highlight::
# 18 passed; 0 failed

CARGO_BUILD_JOBS=2 cargo test -p codegg-tui --lib -- the_default_sets_are_loaded_only_by_the_loader a_semantic_token_request_covers
# 2 passed; 0 failed

CARGO_BUILD_JOBS=2 cargo test -p codegg-tui --test tui_render
# 1250 passed; 0 failed

CARGO_BUILD_JOBS=2 cargo check --workspace --all-targets
# clean

python3 scripts/check_tui_editor_text_authority.py
# passed
```

`scripts/verify.sh scoped` is the roadmap-level closing sweep and is run once
for M001–M006; it is recorded at the roadmap closeout rather than claimed here.

## 5. Corrective found by the line's sweep

The first full `nextest` run of the line executed these tests the way nextest
does — one process per test — and two of them failed:
`a_hit_does_not_reset_the_cache` and
`a_different_path_is_a_different_grammar_and_resets_the_scanner`, both at
"the fixture row must be coloured". The cause was mine: `spans_for_line`'s
first-use kick could win the one load and leave the test observing `None`
for a moment, which a single-process `cargo test` run had hidden because an
earlier test had already loaded the sets. The test loader now delegates to
`wait_for_highlight_assets` and waits, so there is exactly one place in the
file that calls the loaders (the guard test asserts that count is one).
Re-verified: `cargo nextest run -p codegg-tui --lib -E 'test(editor_highlight)'`
— 18 passed, 0 failed.

## 6. Not claimed

- No measured grammar-load wall-clock. The gate is that the draw path cannot
  call the loaders, which the source test above asserts.
- No tree-sitter dependency was added (ADR-0034 Option B holds).
- The cap was not raised.
