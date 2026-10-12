# Desktop Frontend and IDE Foundation Milestone 006-C — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/desktop-frontend-ide-foundation/009-lsp-editor-presentation.md`

Source subsystem roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#m006-decomposition`

Audit of record:

- `plans/subsystems/desktop-frontend-ide-foundation-m006-tui-presentation-audit.md`

Repository baseline reviewed: `db50edb2161ab5c1b14c3aa856745c31414ee6ad`

Implementation commit:

- `359bf291` — `feat(tui): present LSP diagnostics and navigation`

## 1. Executive finding

M006-C presents the daemon-owned LSP read surface in the existing TUI editor.
The editor pulls diagnostics on open, save, and reload; the authorized daemon
pull runs the existing bounded publisher and returns the full tracked
snapshot. The editor shows severity markers and a current-line diagnostic or
an explicit pending/unavailable message, applies bounded visible-line syntax
highlighting, and offers read-only definition/reference navigation through the
active file tree.

The implementation does not add another LSP owner, poller, protocol request,
authorization capability, or write path. In-root result paths are normalized
relative to the daemon-resolved workspace root, external locations are omitted,
and navigation resolves an exact path in the active tree before opening it.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Diagnostics pull is session/project coherent and warm-client-only | `src/core/daemon_lsp.rs`; handler validates session/project binding and uses `find_existing_client_for_root_hint` | pass |
| Pull invokes the existing bounded publisher and returns tracked state | `publish_changed_diagnostics` is called by authorized pull before reading the tracker snapshot | pass |
| Diagnostics load on open/save/reload and stale data clears after edits | editor command/state paths in `src/tui/commands/editor.rs` and `src/tui/app/state/editor.rs` | pass |
| Pending, unavailable, and empty diagnostics remain distinguishable | missing file entry and transport/daemon errors populate visible diagnostics status; empty entry is a valid clean snapshot | pass |
| Severity gutter and current-line diagnostic render | `tui::components::editor::tests::diagnostics_mark_the_gutter_and_show_current_line_message` | pass |
| Syntax styling is bounded to visible lines | editor renderer styles the visible window using the existing syntax highlighter; viewport-bound test remains green | pass |
| Definition/reference navigation stays within the active project tree | exact workspace-relative lookup and existing editor open path; `lsp_navigation_resolves_only_the_exact_workspace_tree_path` | pass |
| Stale asynchronous navigation cannot replace current editor state | editor/navigation generations and pending-navigation guards in editor command completion paths | pass |
| External diagnostic/location paths are omitted | `diagnostic_uri_outside_workspace_is_omitted` | pass |
| Workspace-relative paths preserve nested directories | `diagnostic_uri_keeps_workspace_relative_directories` | pass |
| TUI project and text authority guards | `scripts/check_tui_project_authority.py`; `scripts/check_tui_editor_text_authority.py` | pass |
| Execution/scheduler ownership guards | `scripts/check_execution_ownership.py`; `scripts/check_scheduler_bypass.py` | pass |
| Formatting and repository quick gate | `cargo fmt --all -- --check`; `scripts/verify.sh quick` | pass |
| Focused editor command tests | `cargo test -p codegg --lib tui::commands::editor` — 30 passed | pass |
| Focused editor render tests | `cargo test -p codegg --lib tui::components::editor::tests` — 18 passed | pass |
| Focused daemon path tests | `cargo test -p codegg --lib core::daemon_lsp::diagnostics_path_tests` — 2 passed | pass |
| Authorization and protocol boundary unchanged | no new request, operation, event, or authorization capability; existing `file.read` authority is used | pass |
| Preview-apply write path unchanged | `src/lsp/mutation.rs` is absent from the implementation diff | pass |

## 3. Compatibility and security review

The diagnostics request continues to use the existing `file.read` authorization
and daemon-owned session/project identity. A caller cannot choose the workspace
root. Diagnostic and navigation paths are canonicalized against that root and
only in-root relative paths are returned. The TUI does not perform filesystem
reads during rendering; file activation uses the existing bounded tree and
editor path. Definition/reference operations remain read-only. The existing
preview-apply and dirty-buffer refusal behavior are untouched.

No protocol shape, persistence layout, migration, permission, or dependency
change was introduced. The `LspReadResultDto` path meaning is clarified without
changing its shape.

## 4. Known limitations

- Semantic-token styling is deferred: `LspReadResultDto` does not carry the
  document revision needed to prove that tokens match the current buffer.
- The TUI refreshes diagnostics on open/save/reload; it does not consume the
  diagnostics stream directly. The pull is the correctness authority and runs
  the existing publisher for stream subscribers.
- Native macOS qualification remains a separate outstanding sandbox C004
  requirement; it does not block this milestone's Linux verification.
- Hosted CI and graphical Desktop E2E were not run for this local milestone.

## 5. Closure recommendation

All M006-C acceptance criteria within the repository's available local
verification have passed. The semantic-token limitation is explicit and
preserves document-version correctness. Close M006-C; any revision-aware
semantic-token presentation should be planned with a document revision
contract rather than inferred from a file path.
