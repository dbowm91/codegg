# Desktop Frontend and IDE Foundation — M006 TUI Presentation Audit

Status: complete — M006 is **not** ready as a single implementation handoff; it requires decomposition.

Repository baseline audited: `614e983e5a4f57718ac65d1ecee021719500fd75`.

This record discharges the gate stated in
`plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#milestone-006--tui-ide-vertical-slice`:
*"Complete a fresh TUI presentation audit before writing an M006 implementation handoff."*
It is an audit artifact. It is not a roadmap, not an implementation plan, and not
a closure record.

## 1. Purpose and gate

M006 is registered as *eligible for fresh planning*. This audit answers one
question: what does the TUI actually have today, and is the M006 direction
executable as a single bounded handoff?

Per `plans/003-planning-process.md` §10, an implementation plan is not ready for
handoff until dependency readiness, bounded scope, and unambiguous closure
criteria are answerable. This audit supplies the current-state evidence those
answers depend on.

## 2. Authority and references

- `plans/000-long-term-specification.md` — normative end state
- `plans/001-terminology-and-domain-model.md` — normative language
- `plans/002-long-term-roadmap.md` — macro ordering
- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md` (accepted) — controlling decision
- `plans/adrs/ADR-0010-desktop-frontend-and-shared-client-boundary.md` (accepted)
- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md` — M006 scope
- `plans/closure/editor-document-foundation-post-closure-corrective/001-status.md` — current strict M005-D/M005 authority
- `architecture/document.md`, `architecture/tui.md`, `architecture/lsp.md`, `architecture/client.md`

## 3. Executive finding

**The M005 document foundation is complete and strictly closed, and it has no
consumer.** `codegg_client::DocumentController` and the daemon `document.v1`
surface are implemented, linearized, and verified — but the TUI seam that is
supposed to present them is never constructed in production code.

M006 is therefore not "add an editor widget to an existing vertical slice." It
is "build the first consumer of a finished substrate," and the substrate's
consumers are unevenly supplied:

1. The **document** half of the M005 foundation is ready to present today.
2. The **LSP** half has no frontend contract at all. Every typed LSP read API
   exists in `egglsp`, but **no LSP read operation exists in the native
   `document.v1`-adjacent protocol**, so no TUI code can reach diagnostics,
   hover, definition, references, symbols, completion, or semantic tokens
   without a new daemon and protocol surface.

ADR-0011 §9 deliberately scoped M005's TUI proof to exclude "syntax
highlighting, file explorer, editor panes, terminal composition." Every one of
those excluded items is now M006 scope. The gap is expected, not a regression.

## 4. Work classification

### Invariants (must not regress)

- Daemon owns canonical open-document text; the client is an optimistic replica.
- Disk remains the durable source of truth; saves are checked against a disk base.
- External mutation never silently overwrites a dirty buffer; it becomes a conflict.
- LSP mirrors the document service and is never buffer authority.
- Agent file tools remain disk-authoritative.
- One writer lease per document per transport connection.
- Frontends own presentation state; no second text buffer may exist.
- No projection event may bypass the canonical controller.

### Capabilities (M006 target)

- An editor view over the shared document controller.
- File navigation to open a document.
- LSP-assisted presentation (diagnostics, semantic highlighting, navigation).
- Agent edit/apply/review against saved and dirty documents.

### Infrastructure (must exist before the above)

- A native LSP read protocol surface plus daemon handlers.
- A frontend delivery path for LSP state (currently push-into-internal-state only).
- An editor primary-view layout branch.
- A file-tree data source.

### Polish

- Gutter aesthetics, soft-wrap policy, keymap ergonomics, help overlay text.

## 5. Central finding — the presentation layer does not exist

- `TuiDocumentSession` is declared at `src/tui/mod.rs:1481` and defined in
  `src/tui/document_session.rs`. It is the correct shape: it wraps
  `DocumentController` and holds only `TuiDocumentPresentation { cursor_byte,
  selection, viewport_line }`, with no text buffer.
- **`TuiDocumentSession` is never instantiated in production code.** The only
  other reference in the repository is its own module declaration.
- **`TuiDocumentPresentation` has zero references outside its own file.** The
  cursor/selection/viewport fields are written by its unit test and read by
  nothing.
- `src/tui/components/` contains no `editor.rs` or equivalent buffer widget.
- `architecture/document.md` describes the seam as owning "presentation cursor,
  selection, and viewport **placeholders**." That is accurate: the placeholders
  exist and nothing renders them.

M005-D proved the ownership discipline — that a TUI-kind client can open, edit,
save, conflict, reload, and close through the shared controller without a second
buffer. It proved the contract, not a user-visible editor. Per
`plans/003-planning-process.md` §3, this is infrastructure correctly **not**
represented as completed capability.

## 6. Findings against the expected M006 direction

The roadmap lists seven expected directions. Status against each:

| Expected direction | Status | Evidence |
|---|---|---|
| Editor view over the shared M005 controller | **Seam exists, consumer absent** | `src/tui/document_session.rs:32`; never constructed |
| Workspace/file explorer and search | **Absent** | Sidebar is session/agent telemetry only (`src/tui/components/sidebar.rs:19-27`); no directory traversal in the TUI |
| Diagnostics / completion / navigation from `egglsp` | **Capabilities exist in `egglsp`; no frontend contract** | See §7 |
| Syntax/semantic highlighting and editor motion/keymap | **Absent** | No in-TUI buffer colorizer; no selection/motion/undo model |
| Interactive-process terminal integration | **Partial, not composable** | Renders as plain text lines into a generic info dialog |
| Git/worktree/run/job/agent views from canonical state | **Mixed** | See §8 |
| Agent edit/apply/review against saved/dirty documents | **Blocked by design on dirty buffers** | See §9 |

### 6.1 Widget and layout reality

- Layout is fixed and shallow: `TuiLayout::split` yields `[main, sidebar]`;
  `session_layout` yields `[header, viewport, prompt, footer]`
  (`src/tui/layout.rs:74-103`).
- Primary-view dispatch has exactly three arms — `Route::Home`,
  `Route::Session`, `Route::Workspace` (`src/tui/route.rs:2-12`, dispatched at
  `src/tui/app/render.rs:511-517`).
- Everything else is one of ~50 modal `DialogType` variants rendered above the
  layout via `FocusManager`.
- **There is no pane, split, or focusable-region concept beyond sidebar and
  prompt.** An editor cannot take the viewport without a new layout branch and
  a new focus/region model.
- `ratatui 0.30` with `default-features = false` (`Cargo.toml:143`) supplies no
  gutter, current-line highlight, or per-line style primitive. Line numbers
  exist only in the diff dialog and a read-only source preview.

### 6.2 Editing primitives

- `PromptWidget` is a single `String` with a byte cursor and horizontal scroll
  (`src/tui/components/prompt.rs:10-22`).
- There is **no selection anchor, word motion, kill-line, yank, or undo/redo**
  anywhere in the TUI. `InputMode` is only `Insert | Normal`; the "normal" mode
  is a command mode, not a vi-style buffer mode.
- ADR-0011 explicitly assigns undo/redo to the frontend ("Undo/redo stacks are
  frontend-owned") and defers syntax highlighting/tree-sitter to this milestone.
  Both are greenfield.

## 7. The blocking finding — no LSP read surface exists

This is the most consequential result of the audit and it changes M006's shape.

`egglsp` is substantially more capable than any frontend can currently reach:

- **Diagnostics** — pull APIs exist: `get_diagnostics_for_key`,
  `get_all_diagnostics_for_key`, `get_diagnostic_snapshot_for_key`
  (`crates/egglsp/src/service.rs:2101, 2117, 2144`). Types:
  `FileDiagnostic`, `LspDiagnosticSnapshot`, `LspDiagnosticSource`,
  `LspDiagnosticFreshness` (`crates/egglsp/src/diagnostics.rs:22, 71, 34, 45`).
  Publication is push-only into a `Mutex<HashMap<..>>`
  (`crates/egglsp/src/client.rs:265, 441`).
- **Navigation** — implemented and capability-gated: `go_to_definition`,
  `declaration`, `implementation`, `document_highlights`, `workspace_symbols`,
  `find_references`, `hover`, `document_symbols`, `code_lens`
  (`crates/egglsp/src/operations/navigation.rs:138-464`).
- **Completion** — `operations::completion` / `completion_bounded` and
  `CompletionCandidate` (`crates/egglsp/src/operations/completion.rs:148, 205, 29`).
- **Semantic tokens** — decoded by `decode_semantic_tokens`
  (`crates/egglsp/src/operations/semantic_tokens.rs:41`).

Every one of these is reachable only from the agent tool path
(`src/tool/lsp.rs`) and evidence collection (`src/lsp/semantic_context.rs`).
None is reachable from a TUI view.

**The native protocol exposes no LSP read operation.** A full enumeration of
`codegg-protocol`'s LSP surface yields exactly one LSP variant —
`CoreRequest::LspPreviewApply` (`crates/codegg-protocol/src/core.rs:2486`), a
write. By contrast the M005 document surface is complete and additive
(`DocumentCapabilities`, `DocumentOpen`, `DocumentSnapshotGet`,
`DocumentStatusGet`, `DocumentWriterAcquire`, `DocumentChange`, `DocumentSave`,
`DocumentReload`, `DocumentClose` — `core.rs:1796-1834`).

Consequences for M006:

- LSP presentation is **not** a TUI-local change. It requires a new protocol
  surface, daemon request handlers, authorization, and a frontend delivery
  decision.
- **No push channel exists for LSP state.** Diagnostics land in daemon-internal
  state; the managed document mirror is a one-way daemon→`egglsp` push
  (`src/core/daemon_documents.rs:8-27`), not a broadcast a frontend can
  subscribe to. A frontend must either poll or a new event path must be added.
  The document service explicitly avoids the global `CoreEvent` fanout because
  that fanout lacks a project access filter (`architecture/document.md`), so
  "just add a CoreEvent" would repeat a known access-control defect.
- `LspDiagnosticSource::Pulled` (`crates/egglsp/src/diagnostics.rs:34`) is a
  declared variant with no observed producer — no `textDocument/diagnostic`
  request path is implemented.

## 8. Canonical-state view inventory

- **Canonical projection-backed and reusable:** agent runs and the bounded agent
  tree (64-node cap), tool programs, convergences, and the Task sheet.
- **Canonical request-backed:** worktree list via `CoreRequest::WorktreeList`.
- **Reusable diff machinery:** `DiffViewer` over `similar` with
  `DiffMode::{Inline, SideBySide}` (`src/tui/components/diff.rs:9, 15-18`) is
  ratatui-native and directly reusable for agent edit review. Its gutter logic
  is currently triplicated across three render bodies
  (`src/tui/components/dialogs/diff.rs:70, 180, 332`).
- **`src/tui/file_diff.rs` is statistics only** — additions/deletions, not a
  renderer. Do not mistake it for a diff view.
- **Not canonical:** the git sidebar status is a cached local filesystem/git
  probe, explicitly "cached, not live" (`architecture/tui.md:1109-1110`).
- **Absent:** no job view. `JobScheduler` and `JobSubmissionService` have
  **zero** references anywhere under `src/tui`.

That last point is a change-triggered guard concern, not just a gap. Repository
policy makes `ToolBroker` the only production tool-call boundary and routes
heavy work through `JobSubmissionService` → `JobScheduler`; the existing TUI
instead spawns via `TuiTaskRegistry`/`spawn_blocking`. A new editor view doing
file I/O must add the sanctioned path rather than copy the local precedent, and
`scripts/check_execution_ownership.py` plus `docs/execution-ownership.toml` must
stay in sync.

## 9. Agent edit/apply/review against dirty documents

LSP preview apply is **rejected when any managed document is dirty**, by two
independent gates:

- a clean-path lock taken before the workspace lock
  (`src/lsp/mutation.rs:196-197`), and
- a post-acquisition `is_managed_document_dirty` check returning
  `"... has unsaved editor changes; save and regenerate the preview"`
  (`src/lsp/mutation.rs:240-245`).

Regression coverage: `dirty_document_service_rejects_preview_before_disk_write`
(`src/lsp/mutation.rs:706`, assertion at `:753`).

This is correct and must not be weakened. But it means an M006 editor makes a
real product decision visible: an agent proposing an edit to the file the user
is currently editing unsaved cannot apply it. M006 must either (a) surface the
conflict explicitly and require save-then-regenerate, or (b) introduce an
apply-into-dirty-buffer path — which is a new capability with its own
concurrency and invariant burden, and arguably its own ADR. Choosing (a) is
consistent with ADR-0011; choosing (b) reopens deferred "agent-on-buffer
semantics."

## 10. Reusable seams

| Seam | Location | Use |
|---|---|---|
| `DocumentController` | `crates/codegg-client/src/document.rs` | Buffer text, revisions, conflict states |
| `TuiDocumentSession` | `src/tui/document_session.rs` | Presentation ownership; needs a consumer |
| `document.v1` protocol | `crates/codegg-protocol/src/core.rs:1796-1834` | Open/edit/save/reload/close/status |
| `DiffViewer` + `similar` | `src/tui/components/diff.rs` | Agent edit review rendering |
| `SourcePreviewDialog` | read-only source preview | Bounded read path, existing caps |
| `MessagesWidget` viewport | `src/tui/components/messages.rs` | Scroll/wrap/line-estimation approach |
| `syntect` | `Cargo.toml:174` | Already a dependency; used for message code blocks today |
| `CenteredScroll` | `src/tui/components/scroll.rs` | Minimal scroll clamp only |

## 11. Must-build inventory

Presentation: editor buffer widget; line-number gutter; current-line highlight;
horizontal scroll with soft-wrap continuation; selection model (anchor, drag,
word); vi-style motion and editing commands; frontend undo/redo.

Structure: an editor primary-view route; a layout branch; a focus/region model
beyond sidebar and prompt.

Navigation: a file-tree data source and widget; a document-open path picker.

LSP: a native LSP read protocol surface; daemon request handlers with
authorization; a frontend delivery decision (poll vs new event path);
semantic-token-to-style mapping.

Agent workflow: an explicit conflict presentation for the dirty-buffer preview
gate, or a new apply-into-dirty-buffer capability.

## 12. Documentation defects found

1. **"M006" is ambiguous repo-wide.** `architecture/tui.md:514` uses "M006" for
   *Durable agent-run inspection*, a TUI-subsystem-local milestone. The desktop
   roadmap uses "M006" for *TUI IDE vertical slice*. Both are live labels for
   different work. Planning documents should qualify the subsystem on every
   reference.
2. **"IDE" is ambiguous.** `architecture/ide.md` documents *external* VS Code
   and JetBrains detection plus an MCP diff server (`src/ide/mod.rs`,
   `src/mcp/ide_server.rs`). It has nothing to do with an in-TUI editor, and
   `src/ide/` is its only occupant. M006 planning must not read it as prior art
   for TUI editor work. A terminology note in `plans/001-terminology-and-domain-model.md`
   would remove a real trap — amend only under the §2.1 user-directed/amendment
   conditions, so this is a proposal, not an edit.

## 13. Risks and decision points

| Risk / decision | Severity | Needs ADR? |
|---|---|---|
| M006 as currently written is too large for one handoff (see §14) | high | no — sizing correction |
| LSP read surface design: poll vs new event path, and project-scoped authorization | high | **yes** — a durable protocol/ownership decision |
| Agent apply against a dirty buffer: surface conflict vs new capability | high | **yes** if (b) is chosen |
| Editor reads/writes must use the sanctioned job path, not `TuiTaskRegistry` | medium | no — follow `docs/execution-ownership.md` |
| `Pulled` diagnostics variant with no producer | low | no — either implement or document as reserved |
| Keymap/motion model choice (vi vs modal-minimal) | medium | no — record the decision in the plan |

## 14. Recommended decomposition

`plans/003-planning-process.md` §5 states a milestone is too large when it
"combines several independently releasable capability boundaries." M006 as
written does exactly that: a TUI widget subsystem, a layout/route change, a file
tree, a new protocol surface, an LSP delivery decision, and an agent workflow
decision. It cannot be one bounded handoff.

Recommended split, in dependency order:

- **M006-A — Editor buffer over the shared document controller.** Consume
  `TuiDocumentSession`: buffer render, gutter, current line, selection, motion,
  undo/redo, scroll/wrap, an editor route and layout branch, and a minimal
  document-open path. **No new protocol, no LSP.** This is dependency-ready
  today against strictly-closed M005, and it is the only sub-milestone that is
  immediately implementable.
- **M006-B — Native LSP read surface.** Protocol + daemon handlers +
  authorization for diagnostics, hover, definition, references, symbols, and
  semantic tokens, with the delivery-path decision (ADR).
- **M006-C — LSP presentation in the editor.** Diagnostics gutter/marker
  rendering, semantic highlighting via `syntect` or semantic tokens, and
  navigation actions. Depends on M006-B.
- **M006-D — File tree and explorer.** Depends on M006-A for open/activate.
- **M006-E — Agent edit/apply/review.** Depends on M006-A; the dirty-buffer
  decision belongs here.

A file explorer is a separable user capability and does not need to gate the
first editor. Recommend M006-A alone as the next handoff, and record M006-B
through M006-E in the roadmap so the sequencing is durable.

## 15. Verification implications

- `scripts/verify.sh quick` remains the canonical pre-handoff gate.
- M006-A touches the TUI authority boundary, so `check_tui_authority` and
  `check_execution_ownership` are change-triggered, as is
  `check_websocket_bounds` if M006-B chooses event delivery.
- An LSP read surface is a protocol change and needs a protocol-migration
  statement plus version-skew behavior, consistent with the desktop roadmap's
  existing version-skew section.
- M006-A must retain the M005 invariant that no second text buffer exists; a
  focused guard asserting the editor renders from the controller snapshot is
  warranted, matching the `tui_kind_controller_keeps_edit_through_save_and_reopen`
  trajectory style.

## 16. Disposition

M006 is **not** ready for a single implementation handoff. This audit is
complete and discharges its gate.

Next actions, in order:

1. ~~Update `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md` to record
   this audit and the recommended decomposition, changing the M006 status from
   a single "eligible for fresh planning" slice to an explicitly sequenced
   M006-A…M006-E line.~~ Done.
2. ~~Update `plans/registry.md` to point M006 at this audit.~~ Done.
3. ~~Write the **M006-A** implementation plan under
   `plans/implementation/desktop-frontend-ide-foundation/005-*.md`.~~ Done —
   `plans/implementation/desktop-frontend-ide-foundation/005-editor-buffer-vertical-slice.md`
   is written and was registered as dependency-ready. M006-A needs no ADR.
4. **M006-A has since been implemented** (`1706b50b`, CI step fix `b993522a`,
   PR `#92`) and is conditionally closed at
   `plans/closure/desktop-frontend-ide-foundation/005-status.md`. The static
   guard the plan required, `scripts/check_tui_editor_text_authority.py`, is
   landed, registered in `verify.sh quick` and CI as its own named step, and was
   demonstrated to fail on each violation class it claims to catch. Hosted CI
   `37219495080` has all sixteen guard and lint steps green and Desktop E2E
   `37219495249` is green. The milestone's one deviation — two additive
   read-only `codegg-client` accessors, `try_snapshot` and
   `try_attachment_info` — is approved. The hosted `nextest` sweep was red only
   on a **pre-existing** causal tool-advisor 5 ms wall-clock flake, reproduced
   4/8 on baseline `main` versus 2/8 on this branch and absent from this change
   set. That flake has since been fixed by its own corrective (causal frontier
   timing C001, `cdfd6257`), which changed only the measurement method and left
   the frozen budgets and all assertions intact. **M006-D and M006-E are
   unblocked; M006-B is gated only on its own ADR.**
5. Raise ADRs for the M006-B delivery/authorization decision and, if chosen, the M006-E
   dirty-buffer apply decision. Outstanding.
