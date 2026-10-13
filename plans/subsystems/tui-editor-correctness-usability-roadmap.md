# TUI Editor Correctness and Usability Roadmap

Status: active (planning registered; production implementation not started)

Parent: `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md`, M006-A through M006-E (historically closed).

Baseline: `main` at `6b611b5ca7f019783a91f04530d5ae56124f7ee9`, 2026-10-13. Re-inspect HEAD before each implementation.

Long-term references:
- `plans/000-long-term-specification.md` — TUI-first product, project authorization, correctness before transparent magic, locality.
- `plans/001-terminology-and-domain-model.md` — session, project, workspace and document attachment.
- `plans/002-long-term-roadmap.md` — frontend-neutral projections and TUI/IDE evolution.
- `plans/003-planning-process.md` — milestone sizing, corrective records and handoff.
- `plans/subsystems/editor-document-foundation-roadmap.md` — closed M005 document ownership.

Accepted ADRs: ADR-0008 (LSP preview-apply), ADR-0010 (shared client), ADR-0011 (document ownership), ADR-0012 (warm-only authorized LSP reads), ADR-0018 (safe location identity). Proposed ADR-0021 governs a new completion transport/edit contract; no implementation of that contract is authorized by this roadmap alone.

## 1. Purpose and ownership

Qualify the already-shipped M006 TUI editor for sustained day-to-day code editing: fix the incorrect semantic-token drop policy; bound viewport CPU/memory and directory traversal; then add a revision-safe LSP completion interaction through the daemon and shared document controller.

The TUI owns cursor/focus, viewports, theme presentation, requested completion popup, candidate selection and controller-submitted local edit intentions. `DocumentController` owns the frontend text replica; the daemon's document service owns canonical unsaved content; the daemon-owned LSP service owns language-server interactions, access control and warm/cold readiness. No rendering component gains filesystem, language-server or mutation authority.

## 2. Work classification

**Invariants.** No independent frontend text buffer; no app-side LSP startup; no mutation outside `DocumentController::apply_local`; no path/current-directory-derived authority; no stale completion/semantic result applied to a later revision or tab; all UI work and remote DTOs bounded; model-facing completion remains read-only.

**Capabilities.** Stable semantic highlighting on ordinary source files; responsive horizontal scrolling and large-line behavior; discoverable, bounded, keyboard-operated completion suggestions that insert exactly the approved local edit.

**Infrastructure.** Byte/CPU-budgeted viewport reads and off-runtime filesystem walking; typed LSP completion protocol with revision binding and explicit unsupported/not-ready/stale outcomes; deterministic render and adversarial tests.

**Polish.** Better notices for truncated, unavailable and stale data; coherent Insert/Normal mode and panel-focus behavior; documented keybindings and user-visible failure states.

## 3. Non-goals

No CRDT/OT collaboration, full vim/Helix parity, soft wrap, arbitrary multi-window pane layouts, several simultaneously open document attachments, dirty-buffer agent patch merging, code-action execution, LSP completion commands or automatic import edits. No new generic remote filesystem-browsing protocol: M006-D's local directory listing does not establish remote host availability. No editor/Tauri duplication and no relaxation of sandbox, file.read, or file.modify authorization. These are explicitly deferred successor workstreams, not silently accepted omissions.

## 4. Current-state evidence and defects

M006-C is closed at `plans/closure/desktop-frontend-ide-foundation/010-status.md`, but its closure predates a source-level follow-up:
- `crates/codegg-protocol/src/lsp.rs` allows up to `MAX_LSP_SEMANTIC_TOKENS = 1000`. `src/tui/editor_lsp.rs::validate_semantic_tokens` discards *every* token when count exceeds `MAX_EDITOR_READ_ITEMS = 100` or a legitimate but unstyled token type appears. `src/tui/commands/editor.rs` clears semantic state on edit and accepts revision-matched reads.
- `src/tui/components/editor.rs::line_slice/line_spans` and `src/tui/editor.rs::longest_visible_width` read/measure entire logical lines. Existing tests establish bounded output rows, not bounded bytes scanned or allocations for enormous lines.
- `src/tui/commands/file_tree.rs::rebuild` invokes synchronous `build_tree` inside a Tokio-spawned async task; `src/tui/file_tree.rs` bounds depth to 12 and nodes to 4096, but blocking I/O is not isolated and aborting the async wrapper cannot preempt an in-progress synchronous walk.
- `crates/egglsp/src/operations/completion.rs` has raw `completion` returning LSP items and model-facing `completion_bounded` which *strips* `textEdit`, `additionalTextEdits` and `command`. `LspReadOperation` does not expose completion. The editor currently provides F2–F7 read-only LSP features, not editor completion.
- `src/tui/app/mod.rs` remains a large coordinator; new work must route through existing `TuiMsg` / `TuiCommand` seams rather than add another monolithic input implementation.

Historical M006 closures remain closed. These are **new follow-up findings**, not an assertion that their existing closure evidence did not run. The registry's historical attribution of M006-C to PR #111 is inconsistent with the currently open, unrelated PR #111; preserve immutable closure files and reconcile the provenance separately.

## 5. Target architecture

Semantic token transport and validation are bounded but not destructively conflated with theme coverage: well-formed unknown token types may render plain text; one unsupported style does not erase neighboring valid tokens. The UI reports truncation/staleness and never paints tokens against a mismatched document revision.

A viewport reader consumes bounded text windows derived from the controller snapshot. Length metadata and horizontal display-column indexing may be cached **without retaining a second document text copy**; expensive indexing is amortized or run outside the render path. Pathological lines degrade visibly, not by hanging or losing content without notice.

Filesystem listing remains a scoped navigator and runs in a blocking pool with cancellation checks and generation-checked completion. Explicitly refuse unsupported remote-root access; do not fall back to client cwd or reinterpret a same-spelling local path.

Completion is a separate typed, daemon-authorized read and a **frontend-owned local edit**: a request describes the active authorized attachment and synced revision, the daemon returns bounded inert candidates including supported edit ranges, and the TUI validates selection against its identical current revision before submitting one controller transaction. See proposed ADR-0021; reject undocumented edit execution and no-op defaults masquerading as success.

## 6. Dependencies

`C001 semantic token correctness` and `C002 viewport/tree scheduling`: independent, hard dependency = existing M006 and M005 closures (satisfied). They may be implemented in parallel on separate branches and integrated normally.

`M003 completion contract`: hard gate = ADR-0021 accepted with contract and test scenarios; `egglsp` and `document.v1` interfaces exist, but no stable completion transport contract is currently approved. No implementation until acceptance.

`M004 completion presentation`: hard gates = M003 closed and C001 closed; C002 is a soft integration dependency, but final roadmap closure requires C002 closed as well.

Only a missing actual API/authorization decision should stop work. Do not create serial verification loops where the next implementation is already independently safe to complete.

## 7. Milestones

### C001 — Semantic token fidelity and bounded validation

Plan: `plans/implementation/tui-editor-correctness-usability/001-semantic-token-fidelity.md`. Status: **ready**. Fix legitimate >100-token responses, unknown LSP token kinds, malformed-span handling, theme fallback, UTF-16/Unicode alignment and truncation disclosure. No protocol change. Exit: realistic Rust fixture with >100 tokens highlights supported spans and never erases all styles for one unstyled kind; revision mismatch never paints.

### C002 — Viewport cost and tree scheduling

Plan: `plans/implementation/tui-editor-correctness-usability/002-bounded-editor-render-and-tree-walk.md`. Status: **ready**. Bound line/window scans and allocations, isolate synchronous tree traversal with effective cooperative cancellation, add instrumentation and adversarial tests. No new remote filesystem protocol. Exit: large-line and deep-tree tests measure work bounds and document explicit oversized-content behavior; fast UI input/render remains responsive while slow traversal is underway.

### M003 — Daemon-owned editor completion transport

Plan: `plans/implementation/tui-editor-correctness-usability/003-authorized-lsp-completion-contract.md`. Status: **blocked pending ADR-0021 acceptance**. One bounded read contract and client API with stable attachment/snapshot identity, warm-only LSP, explicit edit support and rejection, and no mutation. Exit: typed, authorized, backwards-compatible completion candidates can be requested against synced document text without starting LSP, exposing secrets outside file.read, or invoking commands.

### M004 — Completion popup, transactional acceptance and qualification

Plan: `plans/implementation/tui-editor-correctness-usability/004-editor-completion-presentation-and-qualification.md`. Status: **blocked on M003 and C001 closure**. Wire a bounded popup, focus and cancellation, candidate preview, safe selection into one controller transaction, and complete adversarial TUI trajectories. Exit: ordinary Rust completion works across daemon/socket mode, preserving undo/redo and refusing stale or unsafe edits; all milestone closure records and integrated tests exist.

## 8. Cross-cutting requirements

**Storage/migration:** No new durable editor state; any protocol expansion is additive and version-compatible. Completion candidates never persist in session/event history as hidden machine authority.

**Authorization:** Every LSP completion request is `via_session` + `file.read` and daemon-owned workspace containment; applying selected text remains an interactive human edit under the existing controller's writer lease. No `LspPreviewApply` shortcut.

**Concurrency/recovery:** Every request is tagged with project, workspace, session, document attachment, revision, generation and request ID as supported by the accepted protocol. Closing/switching/resync invalidates popup and pending edits. Partial results cannot be silently promoted to valid edits. Reconnect re-pulls only on demand; never auto-accept.

**Observability:** Count dropped stale results, validated/rejected tokens, truncated output, render-work/latency and cancelled traversal tasks, without logging document text, snippets or private paths.

**Verification:** Focused unit tests; real editor/controller/socket-daemon trajectories; malformed and multi-byte source tests; slow filesystem/cancel tests; terminal sizes 1x1 and 80x24; root quick verification, workspace fmt/Clippy, and hosted CI on the exact implementation head. One integrated live PTY/interactive TUI smoke is desirable, but if unavailable record it explicitly rather than claiming it passed.

## 9. Known risks and deferred decisions

Semantic token full responses can exceed the daemon cap; do not claim complete highlighting after truncation. Rendering extreme horizontal offsets may require an incremental index instead of naive full-line scans. Tokio abort does not terminate already-running blocking I/O. Completion items may contain snippets, multi-file edits, additional edits or commands: only a deliberately supported inert subset can be locally applied. Remote TUI directory listings need a separate authorization/transport decision. Multiple buffers and pane layout remain a separate IDE successor.

## 10. Completion definition

Close this roadmap only when C001, C002, M003 and M004 have individual closure files with exact commits/CI, the accepted completion ADR is recorded, the regression cases demonstrate expected costs and refusal semantics, and the operator docs explain both supported and unsupported editor features. Do not close merely because code compiles.

## 11. Milestone registry

| Milestone | Status | Plan | Closure |
|---|---|---|---|
| C001 | ready | `plans/implementation/tui-editor-correctness-usability/001-semantic-token-fidelity.md` | pending |
| C002 | ready | `plans/implementation/tui-editor-correctness-usability/002-bounded-editor-render-and-tree-walk.md` | pending |
| M003 | blocked: ADR-0021 proposed | `plans/implementation/tui-editor-correctness-usability/003-authorized-lsp-completion-contract.md` | pending |
| M004 | blocked: M003 + C001 | `plans/implementation/tui-editor-correctness-usability/004-editor-completion-presentation-and-qualification.md` | pending |
