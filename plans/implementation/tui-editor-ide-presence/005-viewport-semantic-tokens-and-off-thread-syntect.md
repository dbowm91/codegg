# TUI Editor IDE Presence Milestone 005 — Viewport semantic tokens and off-thread syntect

Status: closed

Closure record:

- `plans/closure/tui-editor-ide-presence/005-status.md`

Repository baseline: `d585e75f895cf41cd2241512d8d7e7c37fcece99` (re-inspect after M001 has landed, before editing)

Source roadmap:

- `plans/subsystems/tui-editor-ide-presence-roadmap.md#milestone-5--viewport-semantic-tokens-and-off-thread-syntect`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/001-terminology-and-domain-model.md#13-protocol-terms`

Applicable ADRs:

- `plans/adrs/ADR-0034-editor-syntax-tree-dependency.md` (accepted Option B)
- `plans/adrs/ADR-0012-native-lsp-read-surface-delivery-and-authorization.md`
- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`

Primary class: capability

This plan does not reopen `plans/implementation/tui-render-cost/005-editor-draw-path-cost.md`. That milestone closed per-frame draw cost. The 1000-token full-file cap and the render-thread grammar load are a different defect.

## 1. Objective

Semantic color for the visible viewport comes from a bounded range read. Syntect still paints when the server is absent or not ready, and its first grammar load does not run on the render thread.

## 2. Why this milestone is ready

It is not ready to start. The dependency is operational: M001 lands first so this change does not collide in the editor open path. There is no interface dependency on `lsp_sync`. The existing semantic-token pull, the 1000-token cap, and the syntect `Lazy` load are all present at the baseline.

## 3. Current implementation evidence

`request_editor_semantic_tokens` issues `LspReadOperation::SemanticTokens` and then sets `lsp.panel = None`. Highlighting is ambient. `F8` is the retry for `NotReady`. That panel-clear stays. Do not open a result box for a range response.

The daemon read is `textDocument/semanticTokens/full`. `MAX_LSP_SEMANTIC_TOKENS` is 1000, in `crates/codegg-protocol/src/lsp.rs`. The TUI cap equals that constant. An 800-line Rust file exceeds it. The status caveat "highlighting truncated" appears only after a truncated payload arrives. There is no `semanticTokens/range` call in the tree.

Syntect lives in `crates/codegg-tui/src/editor_highlight.rs`. `SYNTAX_SET` and `THEME_SET` are `once_cell::sync::Lazy`, loading `SyntaxSet::load_defaults_newlines` and `ThemeSet::load_defaults` on first use. The first highlighted frame is that first use, on the render thread.

ADR-0034 Option B: no tree-sitter dependency. Syntect is the server-free floor. An LSP semantic span already outranks a syntect span where both exist. That preference stays.

## 4. Invariants that must not regress

- No tree-sitter crate, and no frontend language server.
- One semantic-token response still respects `MAX_LSP_SEMANTIC_TOKENS`. Do not raise the cap so a full-file dump fits.
- A `NotReady` or failed range read leaves the previous validated highlight, or syntect, and does not open `lsp.panel`.
- A stale generation or revision does not replace the current highlight.
- The render path does not call `SyntaxSet::load_defaults_newlines` or `ThemeSet::load_defaults`.
- Syntect remains the floor when the server has no tokens for a viewport line.
- The text-authority guard stays green. The highlighter reads the controller snapshot, not the filesystem.
- The closed draw-path budget (frame byte cap, no full-buffer copy) stays. A range request is not a reason to highlight every line in the file.

## 5. Scope

### In scope

- A viewport range read on the existing semantic-token pull.
- Painting those tokens over syntect for the visible lines.
- Moving the syntect default load off the render thread.
- Keeping the truncation caveat when a single range response hits the cap.

### Explicitly out of scope

- Raising `MAX_LSP_SEMANTIC_TOKENS`.
- Tree-sitter, or a second highlight engine.
- A push of semantic tokens.
- Re-tuning per-frame draw cost from the closed milestone.
- Desktop highlighting.
- A measured grammar-load wall-clock as a gate. The gate is that the render path does not call the load.

## 6. Required production changes

### Core/domain

The language-server operation that today sends `textDocument/semanticTokens/full` gains a range form, `textDocument/semanticTokens/range`, for a viewport the editor names. If the server does not support range, the caller keeps syntect and may issue the existing full request only when the document is within the token cap. It must not issue a full request for a file known to exceed the cap and then drop the tail as the highlight strategy.

The range is in the existing position encoding. It is not a new capability class. It stays on `LspReadGet` / the semantic-token operation. Warm-server-only. `NotReady` is payload-free.

### Storage and migrations

None.

### Protocol and DTOs

The semantic-token read argument may gain a range. `#[serde(default)]` so an old request is the current full read. A response stays inside `MAX_LSP_SEMANTIC_TOKENS`. Truncation is the existing caveat, not a second error type.

Do not add a response field that old clients reject unless this milestone's TUI is the only consumer and the field is defaulted. Prefer a range on the request the TUI already sends.

### Runtime and concurrency

The range follows the viewport. A scroll that moves the viewport schedules another range read, debounced so a scroll burst is one request. The debounce uses the same task channel as other editor LSP reads. A generation or revision change drops the response.

Syntect's `Lazy` initializers move to a load that the editor open path, or a dedicated background task, starts before the first highlighted frame. The render function uses a handle that is either ready or absent. Absent means unhighlighted text plus the gutter, not a blocking load. Do not load inside the draw function.

### Frontend or operator surface

Visible lines prefer a validated semantic span over a syntect span, as today. Lines the range did not cover use syntect. Truncation still sets the existing caveat.

`request_editor_semantic_tokens` still clears `lsp.panel`. A range response is ambient.

### Security and authorization

The range is a read of the open document, on the existing semantic-token capability. The viewport is not a path. No filesystem read.

### Documentation and static guards

`architecture/lsp.md` says interactive highlight is a viewport range, and the full read remains the small-document path. `architecture/tui.md` says syntect load is off the render thread and is the floor. ADR-0034 is unchanged. No new crate in `codegg-tui`'s `Cargo.toml` for parsing.

## 7. Ordered work packages

### Work package A — Range request

Intent: the editor can ask for tokens for a line range, bounded by the existing cap.

Required changes: request argument, daemon operation, `semanticTokens/range` on the wire when the server supports it. Unsupported falls back as §6 says.

Acceptance evidence: a test that a range request is sent for a viewport, and a response larger than the cap is truncated with the existing caveat rather than applied in full.

### Work package B — Paint the viewport

Intent: visible lines use the range tokens; other lines use syntect; a scroll burst is one request.

Required changes: viewport-keyed request, stale response dropped, syntect floor preserved.

Acceptance evidence: a render fixture with semantic tokens on the visible lines and syntect on a line the range did not include. A stale generation does not replace the highlight.

### Work package C — Syntect off the render thread

Intent: the first highlighted frame does not call the default loaders.

Required changes: eager or background load. Draw uses a ready set or skips syntax color. No `Lazy` force on the draw path.

Acceptance evidence: a test or a guard that `editor_highlight`'s draw path does not call `load_defaults_newlines` or `load_defaults`. A first frame before the load completes still paints text.

## 8. Failure, cancellation, restart, and contention semantics

`NotReady`, unsupported range, and transport failure leave syntect (or the previous validated tokens) in place and do not open a panel. Cancellation of a range read on scroll is the superseded request. It must not clear tokens that a newer request has already applied.

Daemon restart drops in-flight range reads through the connection generation. The next viewport request repaints. No token cache is durable.

The syntect load failing is unhighlighted text, not a failed document open.

## 9. Compatibility and migration

An old semantic-token request without a range is the full read, so non-editor callers keep today's behavior. Do not switch those callers to range. Do not raise the cap.

No configuration key. No new crate.

## 10. Required tests

### Focused unit tests

- Range argument round-trips. A missing range is the full read.
- Cap truncation sets the caveat and does not apply the overflow.
- Draw path does not call the syntect default loaders.

### Integration tests

- Viewport request uses `semanticTokens/range` when the server advertises it.
- A server that does not advertise range does not receive a full read for a document over the cap. It receives syntect on the frontend.
- Stale generation does not replace highlight.
- Panel stays closed.

### Restart and recovery tests

None beyond the connection-generation drop the existing read scope covers.

### Contention and cancellation tests

- Two viewport changes in one debounce interval produce one range request.

### Security and negative tests

- The range cannot name a different document. The existing attachment scope is the check.

### Migration and compatibility tests

- A request without a range still performs the full read for a small document.

## 11. Required verification commands

```bash
python3 scripts/check_tui_editor_text_authority.py
scripts/verify.sh scoped
```

Run the egglsp or root tests that own the semantic-token operation, and the TUI render family that owns highlight, from `docs/test-ownership.toml`.

Confirm `codegg-tui`'s manifest did not gain a tree-sitter dependency.

## 12. Documentation updates

- `architecture/lsp.md`: viewport range versus full read, cap unchanged.
- `architecture/tui.md`: syntect load thread and the floor.
- Do not amend the closed draw-path plan. Cite it as a non-overlap.

## 13. Acceptance criteria

- The visible viewport of a large file is requested as a range, not as a full-file dump that the cap then cuts at 1000 tokens.
- The cap is still 1000. A response over the cap is truncated and labeled.
- Syntect paints lines the server did not color, and paints the file when the server is not ready.
- The render path does not load the default syntax or theme sets.
- `lsp.panel` stays closed.
- No tree-sitter dependency was added.

## 14. Stop conditions

- M001 has not landed, and the edit would collide with its open-path change. Wait. Do not fork the open path.
- Range support cannot be expressed on the existing semantic-token read without a new request family or a new capability. Report that. Do not add a push.
- The only way to color an 800-line file is to raise `MAX_LSP_SEMANTIC_TOKENS`. That is a stop. The range is the fix.
- Moving the syntect load requires a new crate or a filesystem read of a grammar the defaults do not include.
- A tree-sitter parse looks cheaper. ADR-0034 Option B forbids it.

## 15. Closure evidence required

- The range-request test and the cap-unchanged assertion.
- The unsupported-range fallback test.
- The draw-path load test or guard.
- The text-authority guard and scoped verify, labeled local.
- A sentence that the closed draw-path milestone was not reopened and the cap was not raised.

## 16. Handoff notes

This milestone does not unblock M006. M006 waits on M001 only. Do not debounce `didChange` here.

Do not edit the protected untracked scripts. Do not reopen ADR-0064 or ADR-0034.
