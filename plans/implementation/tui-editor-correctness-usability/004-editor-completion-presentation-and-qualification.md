# TUI Editor Correctness/Usability M004 — Completion Presentation and Qualification

Status: implemented; closed

Implementation: `fd2faaaf1`
Closure: `plans/closure/tui-editor-correctness-usability/004-status.md`

Prior status was `blocked: M003 completion contract not yet closed`. All hard
gates are satisfied; the M003 `LspCompletionGet` contract is the binding
authority for this milestone.

Baseline: `main` `6b611b5ca7f019783a91f04530d5ae56124f7ee9` (2026-10-13), to be refreshed on activation.
Roadmap: `plans/subsystems/tui-editor-correctness-usability-roadmap.md` M004.
Hard dependencies: `plans/implementation/tui-editor-correctness-usability/003-authorized-lsp-completion-contract.md` closed + C001 closed. Integration/roadmap closure requires C002 closed too.
ADRs: accepted ADR-0011/0012 and proposed ADR-0021 pending acceptance through M003.
Primary class: user-facing capability / reliability / polish.

## 1. Objective

Deliver an explicit, discoverable, keyboard-operated, bounded editor completion popup that displays authorized daemon LSP suggestions, inserts only supported edits into the shared `DocumentController` via the existing single-edit/undo path, and refuses stale/unsafe results under rapid typing, tab switching, disconnect and focus changes. Qualify sustained TUI operation with the editor, conversation, panels and PTY simultaneously available.

## 2. Dependency readiness and current evidence

As of baseline, `src/tui/editor_lsp.rs` stores typed read result/panel state, `src/tui/commands/editor.rs` handles F2–F7 LSP gestures, and `src/tui/components/editor.rs` paints viewport/dialog. `src/tui/editor.rs` owns cursor mode/undo intent and `submit_edit` through `DocumentController::apply_local` records inverse only on acceptance. `src/tui/app/mod.rs::on_key` gives modal dialog, focused side panel, workspace and editor keys distinct precedence. `src/tui/panels/mod.rs` hosts many panels; `/files` is dockable. There is no LSP completion popup or revision-checking UI contract. M003 must first supply that contract; speculative label insertion is prohibited.

## 3. Invariants

- The displayed suggestion never carries mutation authority; one explicit user acceptance, after exact attachment/revision/range verification, is required.
- The editor's buffer is `DocumentController`-owned; completion selection constructs one normal local `TextTransaction` (forward + inverse), not a direct disk write, LSP command, or `LspPreviewApply` request.
- No insert on stale results, lost writer lease, unsupported edit, malformed UTF-16, multi-file/extra edits, server snippet, unfocused panel, wrong session/tab or disconnected transport.
- Normal/Insert/composer/hosted-panel routing and `Esc` semantics remain predictable. Popup must never capture a keystroke meant for an unrelated focused panel.
- Bounded items, label length, popup dimensions, keystroke volume, and retained state; no hidden request polling or durable candidate persistence.
- Preserve C001 semantic presentation, C002 bounds and all existing editor/project static guards.

## 4. Scope and exclusions

**In:** explicit completion action; bounded candidate list/filter/selection/status; request scope+revision state; completion overlay and keyboard/mouse routing; safe insert/replace through controller; undo/redo, stale and conflicting draft behavior; tests and docs.

**Out:** automatic trigger on every character in first cut, snippet expansion, additionalTextEdits, LSP executeCommand, auto-import, fuzzy semantic ranking beyond a deterministic bounded local filter, hover/diagnostic redesign, new editor text cache, multiple editor buffers, arbitrary pane splits and GUI Monaco.

## 5. Required production changes

**Core/document:** no new authority. Reuse controller `apply_local` and bounded undo/redo/inverse mechanisms; make an explicit helper to validate a selected candidate's UTF-16 range against borrowed snapshot and build a single `TextTransaction`. The accepted edit must be atomic and apply once, with one undo unit. Never reconstruct replacement bytes from a guessed prefix if DTO supplies explicit edit.

**Protocol/DTO:** consume M003 types unchanged. If they cannot encode a safe edit or synchronized revision, stop and correct M003 through its closure process rather than infer.

**Runtime:** issue completion only on explicit user action with active editor buffer, valid writable document, resolved session/context and synced snapshot. Bound pending requests; scope by tab, session, workspace, document attachment, editor generation, revision, cursor and request id. Clear popup on edit, cancel, switch, resync, close, session change, disconnect and successful accept. Fail old daemon/unsupported request visibly.

**Frontend:** add `EditorCompletionState` as presentation-only state and a small overlay widget. Display candidate label, kind, clipped detail and applicability reason. Distinguish warming, no suggestions, partial list, unsupported and error. In Insert mode, selected popup intercepts only documented keys; `Esc` dismisses popup and restores the prior editing mode, not quit editor; Enter applies selected supported item, no automatic acceptance of non-applicable items. Use a confirmed crossterm-deliverable default explicit key chord only after checking conflicts; also expose `:complete` (or an equivalently discoverable editor command) so terminal capability variance cannot make the feature unreachable. Never rely on Ctrl+Shift alpha or Ctrl+Space being reliably distinguishable without terminal evidence.

**Security:** no model/agent completion insertion permissions. Untrusted labels/docs never interpreted as escape sequences or unbounded styled markup; render sanitized literal strings and guard terminal output.

**Storage/migrations:** none.

## 6. Ordered work packages

A. Confirm M003 closure and accepted ADR. Define user interaction state table (Insert/Normal/composer/panel, popup open/closed, permissions/revisions) and default keybindings. Add `:complete` discoverability/help and audit conflicts before wiring.

B. Implement a presentation-only completion state and bounded widget. Use existing theme contrast and viewport layout rules; no second text cache. Ensure 1x1/narrow layouts, selection clamping, truncated text and safe untrusted strings.

C. Wire a request through scoped async command with generation/revision identity. Distinguish cold service, stale draft, unavailable old daemon and no actual suggestions. Refuse an applicable edit if any document/cursor/version identity changed after request.

D. Normalize the selected supported edit into a single local transaction; validate UTF-16 surrogate boundaries, replacement range, writer lease and snapshot revision immediately before apply. Record one inverse after success, enable undo/redo, and do not mutate on refusal. Unsupported snippet/additional edit candidate cannot be accepted.

E. End-to-end qualification: daemon/socket TUI with real/rationally stubbed Rust language server; completion while agent streams, while a panel/PTY is active, rapid typing, reconnect, project switch, approval prompt, file-tree open, and huge line. All input stays responsive, permission dialogs keep precedence and no completion text lands in the prompt by mistake. Close roadmap only after C002 also closes.

## 7. Failure/cancellation/restart/contention

While a completion request is pending, subsequent typed edits invalidate it. A stale response may be counted diagnostically but not shown as actionable. Cancelling popup is side-effect free; reconnect clears and needs explicit re-query. On controller rejection or lost lease, preserve the draft and undo history; do not retry automatically or insert elsewhere. Multiple clients editing the same document must respect controller writer/revision semantics. No daemon work persists merely to show a popup.

## 8. Compatibility and migration

No new storage/command authority; user-facing feature is additive. Existing editor keys continue when popup is inactive. Unsupported old daemon displays a clear notice and does not fallback to raw label/guessed prefix. `LspPreviewApply` remains independent. Keep remote attachment locality unaffected.

## 9. Required tests

**Unit:** request gating, key precedence, popup navigation/selection/dismissal, UTF-16→byte conversion, supported simple insertion/replacement, unsupported snippet/additional edits, candidate text escaping/truncation, narrow viewport, theme contrast.

**Integration:** new `tests/tui_completion_trajectory.rs` with actual `App::on_key` → async client request → popup → selected controller transaction → undo/redo. Include wrong tab/attachment/revision, Edit-before-arrival, loss of writer lease, old server, inactive/NotReady LSP, 100/101 candidates, malicious terminal sequences, stream overlap.

**Recovery:** reconnect/tab-switch/close/reopen and stale response after generation changed; no delayed auto-accept or leaks.

**Security:** authorization denial, out-of-bounds UTF-16, multi-file edits, unexpected command and prompt-focus capture failures.

**Compatibility:** existing `tests/tui_panel_docking.rs`, `tests/tui_presentation_regressions.rs`, `tests/document_client_trajectory.rs` and PTY/approval routing remain green.

~~~bash
cargo test --lib tui::editor --locked
cargo test --lib tui::editor_lsp --locked
cargo test --lib tui::components::editor --locked
cargo test --test tui_completion_trajectory --locked
cargo test --test document_client_trajectory --locked
cargo test --test tui_panel_docking --locked
cargo test --test tui_presentation_regressions --locked
cargo test --test interactive_terminal_tui --locked
python3 scripts/check_tui_editor_text_authority.py
python3 scripts/check_tui_project_authority.py
python3 scripts/check_authorization_matrix.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
scripts/verify.sh quick
~~~

Add the new target and confirm existing target names before using them. Require exact-SHA hosted CI and a real or appropriately simulated native TUI keyboard/daemon trajectory; label any unrun live-terminal test explicitly.

## 10. Acceptance criteria

1. A real simple Rust completion displays bounded typed suggestions and inserts only the chosen supported edit.
2. Accepted suggestion equals one controller edit and one undo step; redo replays the intended forward text exactly.
3. Stale/unsynced/wrong-tab/unauthorized/unsafe items cannot modify text, disk or prompt.
4. Normal text input, side panels, approval dialogs, terminal PTY focus, mode switching and `Esc` all remain correct; explicit manual trigger remains reachable without terminal-specific key behavior.
5. Empty Ready differs visibly from NotReady/Stale/Unsupported, candidate truncation is disclosed and no arbitrary LSP command is run.
6. Integration and negative tests plus hosted evidence pass on exact SHA; roadmap closed only when C001/C002/M003/M004 have separate verified closure records.

## 11. Stop conditions

Stop if M003 has not closed, ADR-0021 remains proposed, or controller snapshot/version cannot validate a suggested edit. Do not ship label-insertion fallback, reinterpret unsafe LSP commands, or widen this milestone to snippet/import edits. Keep speculative auto-trigger and full Helix command parity deferred.

## 12. Docs and closure

Update `docs/tui.md` for completion invocation, scope, failure and unsupported behaviors; `architecture/tui.md` for key precedence and undo/revision invariants; roadmap and registry for actual status. Create `plans/closure/tui-editor-correctness-usability/004-status.md` with requirement→evidence matrix, explicit key/PTY scenarios, accepted/refused edits, implementation SHA, hosted CI/terminal evidence, compatibility, unresolved findings and truthful disposition.

## 13. Handoff notes

Completion result is **advisory** until user accepts a validated edit. Favor smallest well-tested popup and edit path over broad new modal/input dispatch logic inside `App`. If a feature requires additional LSP edit variants, plan a successor rather than bypass the narrow M003 contract.
