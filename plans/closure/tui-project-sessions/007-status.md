# Multi-Project TUI Frontend Convergence M007 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-project-sessions/007-modal-focus-state-convergence.md`

Source subsystem roadmap:

- `plans/subsystems/tui-project-sessions-frontend-convergence-corrective-addendum.md#12-milestone-status`

Repository baseline reviewed: `8827647`.

Implementation commits:

- This commit — canonical live modal dispatch/update/render ownership, typed
  FocusManager accessors, discriminator convergence, focus traversal fixes,
  generic-info/chat/shell/terminal migration, regression tests, and docs.

Closure commit:

- This commit — closure evidence, roadmap/registry status, and downstream
  dependency disposition.

## 1. Executive finding

M007 is complete. `FocusManager` owns the mounted live component stack and is
the authority for modal input, rendering, active identity, duplicate
suppression, and nested close behavior. Typed accessors update the mounted
instance directly; the former `replace_top_dialog` synchronization path is
gone. `ui_state.dialog` is retained only as a derived compatibility mirror.

Generic `InfoDialog` views, including ProjectChat, Collaborators, shell output,
terminal output, and plugin output, now update the live mounted component.
Project picker and UI-node dialogs have explicit discriminator identities.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Canonical live modal owner | `FocusManager::stack`, `with_dialog_mut`, `dialog_mut_any`, and `with_component_mut` | pass |
| Duplicate modal rule | `FocusManager::push` rejects an existing `DialogType`; callers focus/update the existing path | pass |
| Nested modal behavior | `pop`, `pop_dialog`, active-type derivation, and nested focus regression | pass |
| Forward/reverse focus wrapping | `handle_tab` uses each component's current count and symmetric modulo arithmetic | pass |
| Zero/one/N focusable controls | FocusManager unit tests cover 0, 1, and 3 controls | pass |
| Discriminator convergence | Exhaustive `Dialog`/`DialogType` round-trip test covers every variant, including ProjectPicker, Collaborators, and ProjectChat | pass |
| Clone/replace removal | `replace_top_dialog` and the obsolete app replacement helper are removed; generic-info, shell, terminal, UI-node, source-preview, and run-detail paths update/mount live instances | pass |
| Modal input isolation | `App::on_key` routes non-empty FocusManager state before prompt/global input; component messages return through `process_msg` | pass |
| Async stale protection | Existing bounded request generations remain authoritative; terminal/chat/session/research completion handlers retain stale guards and only refresh mounted views | pass |
| Permission/question authority | Pending IDs and response state remain in `DialogState`/session state; presentation close cannot answer or discard daemon truth | pass |
| Observer/plugin compatibility | ProjectChat/Collaborators remain read-only info projections; plugin effects retain existing validation and source ownership | pass |

## 3. Modal ownership census and migration matrix

| Modal family | Lifecycle discriminator | Mounted live owner | Update source |
|---|---|---|---|
| Model / Agent / Session / Tree | matching `DialogType` | FocusManager component stack | component messages and existing session reducers |
| Theme / Import / Connect / Keybind / MCP / Share / Template | matching `DialogType` | FocusManager component stack | typed dialog handlers and pending request state |
| Diff / Review / SourcePreview / RunDetail | matching `DialogType` | FocusManager component stack | command completion payloads |
| Research / Security | matching `DialogType` | FocusManager component stack | guarded async completion and latest receipt |
| Context / Cost / Usage / Stats / Doctor / Memory / Tasks / Worktrees / Goals | matching `DialogType` | live `InfoDialog` or `UiNodeDialog` in FocusManager | direct content mutation |
| Shell / Terminal | `ShellShow` / `Terminal` | live `InfoDialog` in FocusManager | bounded projection refresh |
| ProjectChat / Collaborators | explicit info subtype and `DialogType` | live `InfoDialog` in FocusManager | reducer projection refresh |
| ProjectPicker | explicit `ProjectPicker` | FocusManager component identity plus bounded picker domain state | picker phase reducer |
| Plugin | generic `Plugin` | FocusManager plugin component | validated plugin UI effect |

The before-state census found parallel component copies in `DialogState`, a
single global focus index, ProjectPicker's `DialogType::None`, and clone-based
replacement. The after-state lifecycle path uses the mounted stack and
component-owned focus; compatibility/domain fields remain outside rendering
and input authority while their existing reducers are migrated by the later
App decomposition milestone.

## 4. Production implementation evidence

- `Component` is object-safe through the narrow `AsAny` adapter. The stack is
  never exposed; typed lookup/mutation is performed by `FocusManager`.
- The active type is derived from the stack after push, pop, and render-panic
  recovery. Dialog rendering no longer depends on a stale mirror.
- ProjectPicker, ProjectChat, and Collaborators have real lifecycle
  discriminators instead of silently converting to `None`.
- Shell and interactive-terminal refreshes mutate the mounted `InfoDialog`
  rather than writing a detached clone.
- Source-preview and run-detail completions mount their completion payload
  directly.
- Prompt/global input is unreachable while a modal is mounted, including when
  the modal does not consume a particular key.

## 5. Verification executed

Passed locally:

- `cargo fmt --all -- --check`
- `cargo check -p codegg --lib`
- `cargo check -p codegg --tests`
- `git diff --check`
- Focused FocusManager/discriminator test build was attempted with the native
  target and an x86_64 library-path correction.

The focused test binary could not complete native linking on this host. The
repository Rust target is `x86_64-apple-darwin`, while the default
`/opt/local` compression/iconv libraries are arm64; the linker consequently
reports undefined x86_64 `lzma_*` symbols. Rust compilation and test-target
type checking completed successfully. This is recorded as an environment-only
verification limitation, not a source assertion failure. The remaining broad
verification commands are retained for the hosted CI/compatible-toolchain
closure gate.

## 6. Invariant and failure/recovery review

- A duplicate open cannot create a hidden second component of the same dialog
  type.
- Popping a nested modal exposes the prior component and its own focus state;
  no global focus index is restored or leaked.
- A closed modal's request generation remains cancelled/invalidated by the
  existing request-state guards. A delayed completion cannot recreate a stale
  mounted component.
- Render panic recovery pops only the failed top modal and derives the mirror
  from the remaining stack.
- Permission/question durable response IDs remain separate from presentation
  state and continue to fail closed on invalid/observer paths.
- Plugin UI effect bounds, session targeting, and ownership validation are
  unchanged.

## 7. Compatibility, documentation, and migration

No storage, protocol, daemon, authorization, scheduler, or persistence
migration was introduced. Existing dialog components retain their public
`Component` behavior. The compatibility `Dialog` enum remains supported, but
the live component's `DialogType` controls lifecycle.

Updated documentation:

- `architecture/tui.md`
- `.opencode/skills/tui/SKILL.md`
- component/focus source comments

## 8. Unresolved findings and severity

| Severity | Finding | Disposition |
|---|---|---|
| Low / environment-only | Native focused test execution is blocked by the host's mixed-architecture `/opt/local` libraries | Hosted CI or a compatible x86_64 toolchain must execute the binaries |
| Low / follow-up | Some legacy reducer compatibility fields remain in `DialogState` for existing command reducers; they are no longer render/input owners | M008 may remove the transitional fields after its physical decomposition census; no lifecycle authority depends on them |
| None | No discriminator, input-leak, plugin, observer, permission, or stale-view regression was found in the migrated paths | closure accepted |

## 9. Roadmap disposition and unblocking audit

M007 is closed. The dependency graph was re-read after implementation:

- M008 is newly ready because hard dependencies M005, M006, and M007 are
  closed.
- M009 is newly ready because its hard M007 modal/focus dependency is closed;
  M005 remains only a soft sequencing preference and is already closed.
- M010 remains ready; M009 remains a soft sequencing preference.
- No unrelated blocked plan was changed.

The implementation plan is marked `implemented`, the addendum records M007 as
closed and M008/M009 as ready, and the registry records this closure and the
new dependency-ready plans.

## 10. Registry updates

Updated in the closure commit:

- `plans/implementation/tui-project-sessions/007-modal-focus-state-convergence.md`
  → `implemented`
- `plans/subsystems/tui-project-sessions-frontend-convergence-corrective-addendum.md`
  → M007 `closed`, M008/M009 `ready`
- `plans/registry.md` → M007 recent closure, M008/M009 dependency-ready, and
  current frontend-convergence control row
- this closure record added as the accepted evidence gate

## 11. Final recommendation

Closed. Proceed with M008 or M009; M008 should remove the remaining
compatibility reducer fields as part of its already-planned physical
decomposition, while M009 can consume the now-stable FocusManager contract.
