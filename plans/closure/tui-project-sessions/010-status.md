# Multi-Project TUI Frontend Convergence M010 — Closure Status

Status: closed

Source implementation plan: plans/implementation/tui-project-sessions/010-command-discovery-keybinding-convergence.md
Source subsystem roadmap: plans/subsystems/tui-project-sessions-frontend-convergence-corrective-addendum.md#7-milestones
Repository baseline reviewed: 98bc89fa613f5a1390202a90b497f59d5732d431

Implementation commits:

- 876a956b9e9cf9419bb0f7c06412dd88b88dc7d2 — converge command discovery and keybindings

## 1. Executive finding

M010 is complete. All 139 built-in slash commands now receive declarative
user-facing domain, scope, source, and searchable keyword metadata. The palette
searches the active scoped catalog across names, aliases, descriptions, domains,
and keywords, and shows bounded source context for dynamic entries. Legacy
command strings, aliases, categories, parser, and dispatcher remain intact.

The configurable keybinding surface has one exhaustive 47-entry
ActionKey/ActionDescriptor catalog. The only InputAction excluded is Char,
explicitly documented as ordinary prompt text. Editor, diff, command,
project-tab, and sidebar actions are covered. Import/export use the same
snake_case representation, and remapping rejects collisions rather than
silently overwriting another action.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Full command/action census | 139-command domain table and 47-action list below; Char is explicit exception | pass |
| Canonical command metadata | CommandDomain, CommandScope, CommandSource, keywords, registry validation tests | pass |
| Active-project palette | test_command_palette_switches_active_project_catalog and scoped registry tests | pass |
| Bounded fuzzy discovery | Metadata-aware filter retains ten-result cap; palette performs no I/O | pass |
| Deterministic collisions | Sorted project/plugin additions, alias-aware reservation, validation tests | pass |
| Keybinding coverage | ActionKey::all, ActionDescriptor, exhaustive coverage test | pass |
| Keybinding round trip | Snake_case export/import and external_editor/open_diff parsing tests | pass |
| Collision safety | Shared bind_waiting_key path and remap collision test | pass |
| Help/keybinding convergence | Keybind dialog and help overlay consume canonical action labels; docs updated | pass |
| Slash compatibility and one router | Existing dispatch unchanged; built-in/alias tests and full TUI suite | pass |

### Command census

Every Command::new built-in in src/tui/command.rs is covered by the classifier;
dynamic entries use the same metadata plus explicit source/scope provenance.

| Domain | Count | Canonical command names |
|---|---:|---|
| Agent | 5 | agents, agent, security-review, security-review-show, security-review-cancel |
| Collaboration | 15 | collaborators, observe, stop-observing, chat, chat-send, chat-reply, chat-history, chat-sync, chat-read, chat-edit, chat-redact, chat-composing, chat-action-task, chat-action-review, chat-action-list |
| Diagnostics | 28 | status, doctor, lsp-status, lsp-previews, lsp-preview, lsp-preview-clear, lsp-preview-refresh, lsp-preview-apply, lsp-servers, lsp-capabilities, lsp-errors, lsp-root, lsp-restart, lsp-stop, lsp-cache-status, lsp-cache-clear, lsp-doctor, lsp-context-diagnostics, lsp-repair-local, lsp-repair-hunk, lsp-review-file, lsp-review-diff, lsp-security-review, lsp-impact, lsp-test-repair, lsp-interface, lsp-cross-repair, lsp-call-neighbors |
| Execution | 27 | loop, tasks, task-del, checkpoint, goal, plan, state, tests, test, shell-list, shell-show, shell-include, shell-rerun, shell-kill, shell-ask, shell-expand, terminal-create, terminal-list, terminal-attach, terminal-show, terminal-focus, terminal-send, terminal-resize, terminal-resume, terminal-detach, terminal-terminate, terminal-remove |
| Git/Review | 5 | pr, issue, review, diff, revert |
| Memory | 11 | memory, memory-search, memory-list, memory-remember, memory-forget, memory-consolidate, habits, habit-dismiss, skill-promote, skill-proposals, skill-proposal |
| Project | 3 | workspaces, tree, editor |
| Provider | 6 | connect, connections, models, models-refresh, mcps, tool-backends |
| Research | 5 | research, research-runs, research-open, research-show, search |
| Session | 18 | sessions, new, share, unshare, rename, compact, timeline, fork, undo, redo, export, import, timestamps, thinking, context, cost, usage, stats |
| System | 16 | exit, themes, help, reload, variants, keybinds, tui, tts, tui-stats, plugins, plugin-info, plugin-enable, plugin-disable, plugin-doctor, plugin-remove, plugin-install |
| Total | 139 | Every built-in command |

### Configurable action census

The 47 canonical descriptors are: Send, Newline, Cancel, NavigateUp,
NavigateDown, SwitchAgent, SelectModel, ClearSession, NewSession, ToggleSidebar,
FocusSidebar, ToggleSection, CloseSession, Help, FocusPrompt, StashPrompt,
RestorePrompt, CopyMessage, CycleModelForward, CycleModelBackward,
ToggleReasoning, Quit, ExternalEditor, Backspace, Delete, Left, Right, Home,
End, PageUp, PageDown, Search, SearchNext, SearchPrev, ClearSearch, Command,
ToggleTts, StopTts, ToggleFullscreen, TogglePermissionMode, OpenDiff, GoToTop,
GoToBottom, OpenProjectPicker, NextProjectTab, PreviousProjectTab, and
CloseProjectTab. InputAction::Char(char) is the sole explicit non-configurable
exception because it represents ordinary prompt text.

## 3. Production implementation evidence

- src/tui/command.rs adds stable domain, scope, source, and keyword metadata
  while preserving CommandCategory and all execution fields. Built-ins are
  built-in/global; config commands are config/global; project files are
  project-scoped; plugin entries expose a bounded plugin source label.
- Project additions are sorted before append. Existing entries win
  name/alias collisions, plugin aliases are checked, and validate() rejects
  ambiguous catalogs.
- The palette renders domain and dynamic source context and searches the
  scoped catalog without filesystem work.
- src/tui/input.rs owns the exhaustive action catalog and direct mappings to
  InputAction. The keybind dialog consumes it and exports config-compatible
  JSON; the app-level handler uses the same collision-safe operation.
- architecture/command.md, architecture/tui.md, and .opencode/skills/tui/SKILL.md
  document the converged contracts.

No storage, protocol, daemon, scheduler, authorization, or command-dispatch
authority was added or duplicated.

## 4. Verification executed

The host's default x86_64 linker discovers an arm64 MacPorts liblzma. Focused
test commands therefore set LZMA_API_STATIC=1, using the crate's bundled static
fallback without changing production behavior.

```text
LZMA_API_STATIC=1 cargo test -p codegg --lib tui::command::tests:: --no-fail-fast
LZMA_API_STATIC=1 cargo test -p codegg --lib tui::input::tests:: --no-fail-fast
LZMA_API_STATIC=1 cargo test -p codegg --lib tui::components::dialogs::keybind::tests:: --no-fail-fast
LZMA_API_STATIC=1 cargo test -p codegg --lib tui::components::dialogs::command --no-fail-fast
LZMA_API_STATIC=1 cargo test -p codegg --test tui --no-fail-fast
LZMA_API_STATIC=1 cargo check -p codegg --tests
cargo fmt --all -- --check
LZMA_API_STATIC=1 CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features -- -D warnings
LZMA_API_STATIC=1 CARGO_BUILD_JOBS=1 scripts/verify.sh quick
```

Results: command 9 passed; input 31 passed; keybinding 3 passed; command
dialog 8 passed; full TUI integration 165 passed; cargo check passed;
all-feature/all-target Clippy passed with zero warnings/errors; and quick
verification passed all existing guards and workspace checks. An initial
unconfigured x86_64 test link was blocked only by the host liblzma mismatch;
the static fallback run passed the focused tests.

## 5. Invariant review

- Slash strings and aliases remain unchanged; no command was renamed or removed.
- The existing parser/dispatcher remains canonical; palette/help are discovery-only.
- Metadata does not authorize execution or bypass observer, daemon, or permission gates.
- Active-project catalogs continue to come from M005's explicit workspace context.
- Filtering is bounded and performs no I/O; dynamic source text is bounded before rendering.
- Backend concepts such as goals, plans, tasks, runs, schedules, and agent runs remain distinct.
- Keybinding mappings are deterministic, exhaustive, snake_case round-trippable, and collision-checked.

## 6. Failure and recovery review

This milestone adds no asynchronous or durable operation. Catalogs rebuild from
existing runtime assets on startup/tab activation and do not create indexes.
Incomplete metadata and ambiguous collisions fail validation. Invalid imports
retain the existing diagnostic path; conflicting remaps leave the old binding
intact. No cancellation, restart lease, generation, or persistence semantics
were introduced.

## 7. Migration and compatibility review

No schema or protocol migration is required. Legacy category values, command
names/aliases, project/plugin formats, and keybinding config syntax remain
supported. Export now emits the bindings wrapper and snake_case enum names
accepted by import. New metadata is frontend-only and recomputed.

## 8. Security review

Discovery metadata is not an authority boundary. Existing daemon authorization,
observer fail-closed checks, permission handling, and plugin execution paths
remain canonical. Source labels are bounded before rendering, and palette
filtering introduces no secrets or filesystem/network work.

## 9. Documentation and operations

Updated architecture/command.md, architecture/tui.md, .opencode/skills/tui/SKILL.md,
the implementation/roadmap/registry status surfaces, and this record. No new
CI lane, scanner, persistence store, telemetry, or operator service was added.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | The default local x86_64 linker needs LZMA_API_STATIC=1 because discovered MacPorts liblzma is arm64. | Environment-specific local test inconvenience; source and CI checks pass. | Keep the invocation note and use the existing compatible-host/CI path when required. |

No critical, high, or medium findings remain.

## 11. Roadmap disposition

Milestone closed. M005-M010 closure records are now complete, so the corrective
addendum roadmap can move to closed. No corrective pass is required.

## 12. Registry updates

- Move M010 from active implementation to recently closed with commit
  876a956b9e9cf9419bb0f7c06412dd88b88dc7d2.
- Remove its dependency-ready/active rows and close the corrective roadmap.
- Audit of plans/registry.md blocked work and affected dependency graphs found
  no registered plan whose hard or interface dependency is M010. Existing
  Architecture M009 and Runtime Safety C002 blockers are unrelated and remain
  blocked; no future plan can be unblocked by this closure.
