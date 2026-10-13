# TUI Editor Correctness/Usability C001 — Semantic Token Fidelity

Status: ready for handoff

Baseline: `main` `6b611b5ca7f019783a91f04530d5ae56124f7ee9` (2026-10-13); inspect HEAD again before editing.
Roadmap: `plans/subsystems/tui-editor-correctness-usability-roadmap.md` C001.
Predecessors: M005 document core, M006-B native LSP reads (`plans/closure/desktop-frontend-ide-foundation/009-status.md`), M006-C editor LSP (`plans/closure/desktop-frontend-ide-foundation/010-status.md`).
ADRs: accepted 0011, 0012, 0018. No new ADR: repair within existing read contract.
Classification: correctness invariant / corrective.

## 1. Objective

Prevent ordinary valid semantic-token responses from losing *all* highlighting because they exceed 100 entries or contain one legitimate unstyled LSP token kind. Enforce Unicode/overlap/revision safety and disclose partial/truncated results.

## 2. Dependencies and current evidence

Hard predecessors closed. `crates/codegg-protocol/src/lsp.rs::MAX_LSP_SEMANTIC_TOKENS` is 1000 and `src/core/daemon_lsp.rs` caps and reports truncation at that number. `src/tui/editor_lsp.rs::MAX_EDITOR_READ_ITEMS` is 100; `validate_semantic_tokens` immediately discards responses above that cap. It also rejects the entire set for token types not in the narrow styling whitelist, including ordinary `variable`, `parameter`, `property` and `namespace` types. `src/tui/components/editor.rs::known_token_style` has a separate partial whitelist and fixed colors. `src/tui/commands/editor.rs::apply_editor_lsp_read` already guards generation and snapshot revision.

Previous M006-C tests establish malformed range and stale-result refusal, not positive mixed-kind fidelity on realistic >100-token Rust source. Preserve previous closure; create new corrective evidence.

## 3. Invariants and non-goals

Only `DocumentController` owns text; no frontend text cache or second buffer. LSP data cannot mutate text. Only the exact current session/project/workspace/attachment/generation/revision can decorate a snapshot. UTF-16→UTF-8 conversion must reject split surrogates, out-of-range and overlapping spans. Unknown *style* is not a malformed range. A truncated result is not represented as a complete file highlight. Retain warm-only service, `file.read` and existing `LspPreviewApply` ownership.

Out: new protocol; completion; new syntax engine/Tree-sitter; unlimited semantic tokens; semantic push; diagnostics publisher; multi-buffer editor; changing agent mutation.

## 4. Required production changes

**Core/domain:** none. Use existing protocol maximum `MAX_LSP_SEMANTIC_TOKENS` as the frontend's received-token cap; distinguish panel-item cap from semantic-token cap.

**Storage/protocol:** no migration, DTO changes or new authorization descriptor.

**Runtime:** retain request/generation + snapshot revision validation. Clear stale semantic state on local edit, reload, resync, attachment switch and reconnect; no server calls on render path.

**Frontend:** separate (a) structural token validation, (b) style-class mapping, (c) bounded drawing. For well-formed but unknown token types, use plain text and continue painting other supported tokens. Malformed spans may be dropped individually if safe without changing later offsets; globally incoherent responses fail closed with visible not-ready/incomplete state. Preserve `truncated` in presentation and show an incomplete-coverage signal. Map supported token classes through theme slots/legible fallback rather than hardcoded colors. Never treat Ready with zero tokens as NotReady.

**Security:** no path or document authority broadened; continue passing `scripts/check_tui_editor_text_authority.py`.

## 5. Ordered work packages

A. Reproduce failure with 101, 500 and 1000 tokens, mixed Rust token kinds and `truncated=true`; prove pre-fix all-or-nothing loss. Add a fixture that includes a valid UTF-16 supplementary character and known styled tokens on each side of an unstyled token.

B. Replace the shared 100-item read-panel threshold with correct semantic cap; validate sorted non-overlapping Unicode spans from a borrowed snapshot. Record why and when one token versus whole result is rejected. Ensure no token becomes a write effect.

C. Update `known_token_style` / line-spans to preserve valid neighboring styles, theme contrast, current-line/diagnostic indicators and horizontal clipping; expose truncation explicitly.

D. Write unit/renderer/trajectory tests, exercise stale responses after edit/tab switch and close, update docs and create new closure record.

## 6. Failure/cancellation/restart/contention

Requests remain non-authoritative and bounded. A late completion after edit, resync, reconnect or switch is discarded. A malformed/unauthorized token cannot paint a different document or cause index panic. No automatically retried speculative token response. On truncated data, communicate incomplete coverage; do not invent tokens.

## 7. Compatibility

Additive frontend behavior only. No new storage, protocol, server startup or config. Existing clients and daemons remain interoperable. Keep M006 historical closure immutable.

## 8. Tests and verification

Tests required: mixed standard token types, unknown types, count 101/500/1000 and >cap, zero Ready vs NotReady, truncated response, malformed line/offset/surrogate/overlap, theme visibility, unknown token preserving supported spans, stale generation and revision, reopened same path.

~~~bash
cargo test --lib tui::editor_lsp --locked
cargo test --lib tui::components::editor --locked
cargo test --test lsp_read_surface_trajectory --locked
cargo test --test document_client_trajectory --locked
python3 scripts/check_tui_editor_text_authority.py
python3 scripts/check_tui_project_authority.py
python3 scripts/check_authorization_matrix.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
scripts/verify.sh quick
~~~

Check existence/current names of targeted suites before running and record substitutes; never claim tests not run. Hosted root CI and relevant TUI renderer/terminal tests on the same SHA are required for strict closure.

## 9. Acceptance and stop conditions

Must demonstrate valid Rust >100-token response paints known styles and keeps unknown types plain; no whole-response discard from 100-item local cap; Unicode-safe, bounded, stale-safe and theme-legible results; truthful truncation; unaffected file bytes; all guards/tests recorded. Stop and raise an architectural issue only if the correction would require new protocol authority, whole-document TUI text storage or cross-cutting LSP ownership changes. Do not expand into completion.

## 10. Documentation and closure

Update `architecture/tui.md` and `docs/tui.md` with style/limit/truncation semantics. Register actual progress and create `plans/closure/tui-editor-correctness-usability/001-status.md` mapping each acceptance requirement to focused tests, before/after positive fixture, exact SHAs/CI, failure cases, security checks, residual severity and a truthful disposition.

## 11. Handoff notes

A small validator + renderer correction with realistic positive cases is preferable to another syntax-engine abstraction. Do not assume 1000 file-wide tokens means complete coverage of a large document.
