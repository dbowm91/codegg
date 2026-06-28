# TUI Phase 12: UX Consistency and Discoverability Polish

## Objective

Polish TUI usability now that the runtime is more responsive and reliable. This phase should make state, actions, background work, shell output, dialogs, and agent activity easier to understand without adding heavy new architecture.

The emphasis is consistency: the same concepts should be named the same way everywhere, status should be visible without guesswork, and long or structured output should not be forced into fragile toasts.

## Current Shape

The TUI now has stronger foundations:

- async command start/apply paths
- async file diff sidebar updates
- terminal guard
- diagnostics and `/tui-stats`
- mode-aware help entries
- shell detail view and killed status
- component-level render fallbacks

The remaining UX issues are mostly about presentation and discoverability rather than core correctness.

## UX Principles

1. Toasts are for short transient notifications, not long structured output.
2. Dialogs should have explicit titles, states, footers, and close behavior.
3. Status bar should communicate current operating mode and background activity.
4. Labels should be consistent across header, sidebar, dialogs, commands, and docs.
5. Destructive actions should show clear pending/success/failure state.
6. Power-user shortcuts should be discoverable, mode-aware, and accurate.

## Workstream 1: Move Long Output Out of Toasts

### Problem

Several commands still use toasts for multi-line or structured output. This can be unreadable, clipped, hard to scroll, and visually noisy.

Candidates:

- `/tui-stats`
- doctor output
- task list
- worktree list
- shell list if more than a few entries
- memory search results
- goal show/budget output
- bulk operation partial-failure details
- diagnostics/warnings with multiple lines

### Design

Introduce a generic read-only info dialog if not already sufficient:

```rust
InfoDialog {
    title: String,
    info_type: InfoType,
    lines: Vec<String>,
    scroll: usize,
    footer_hints: Vec<Hint>,
}
```

If an `InfoDialog` already exists, standardize these commands on it.

### Rules

- Toast if content is one line and short.
- Info dialog if content has multiple lines, structured fields, or may exceed ~120 chars.
- For operations that need both: show short toast `Doctor report ready` and open dialog with details.

### Acceptance Criteria

- `/tui-stats` opens or can open a scrollable details view.
- Task/worktree/shell list long output is available in a scrollable surface.
- Toasts remain short and transient.

## Workstream 2: Status Bar State Normalization

### Problem

The status bar has many possible sources of truth: idle/working/error, streaming, tool-running, permission pending, question pending, background tasks, shell command running, security review, research loading, memory operation, model/agent, LSP state, and token usage. These can conflict or be invisible.

### Design

Create a single status composition function:

```rust
pub struct TuiStatusSummary {
    pub primary: String,
    pub secondary: Option<String>,
    pub activity: Vec<String>,
    pub warning: Option<String>,
}

impl App {
    pub fn build_status_summary(&self) -> TuiStatusSummary;
}
```

Priority order for primary state:

1. render error / degraded component if active
2. permission pending
3. question pending
4. security review running
5. agent working / streaming
6. shell running
7. background task active
8. idle
9. error state if no more specific state applies

Secondary/activity chips can include:

- `model:<name>`
- `agent:<name>`
- `lsp:<state>`
- `tasks:<n>`
- `shell:<n>`
- `diff:<n pending>`
- `mem`
- `research`

### Acceptance Criteria

- Status bar text is generated from a single summary builder.
- Permission/question/security/streaming/background states are visually distinct.
- `/tui-stats` and status bar agree on active task counts where applicable.

## Workstream 3: Naming and Label Consistency

### Concepts to standardize

Use consistent labels:

- `session`, not alternating conversation/chat/session unless intentionally distinct
- `turn`, for model interaction unit if used
- `agent`, for main agent/persona
- `subagent`, for delegated worker
- `model`, for model route/name
- `provider`, for provider prefix
- `goal`, for long-horizon objective
- `task`, for scheduled/background task
- `shell command`, for human shell execution
- `tool call`, for model/tool invocation
- `diff`, for file-change statistics or hunk preview
- `permission`, for human approval state

Audit:

- header
- status bar
- sidebar
- help dialog
- command palette
- shell list/show/ask text
- session dialog
- task/worktree/memory/goal toasts and dialogs
- architecture docs and skill docs

### Acceptance Criteria

- User-facing labels are consistent across core TUI surfaces.
- Ambiguous terms are documented if intentionally distinct.
- Tests or snapshot-ish assertions cover a few key labels if practical.

## Workstream 4: Dialog Footer and Help Consistency

### Problem

Mode-aware help improved global shortcuts, but dialog-local hints can still drift from actual behavior.

### Design

Add a small footer-hints abstraction:

```rust
pub struct DialogHint {
    pub key: &'static str,
    pub label: &'static str,
}

pub trait DialogHints {
    fn hints(&self) -> &[DialogHint];
}
```

Or keep it simpler: each dialog exposes `footer_text()` generated from a common helper.

Standard hints:

- `Esc close`
- `Enter select/confirm`
- `↑/↓ move`
- `Tab next field` only where implemented
- `/ search` only where implemented
- `? help` only where implemented

### Targets

- model selector
- session selector
- tree dialog
- import dialog
- permission dialog
- question dialog
- research browser
- shell show/info dialog
- security review dialog
- help dialog itself

### Acceptance Criteria

- Dialog footer hints match actual key handling.
- Insert/normal/global help does not claim dialog-local keys globally.
- Tests cover at least three representative dialogs.

## Workstream 5: Shell UX Polish

### Goals

Shell execution is now more correct. Make it easier to use repeatedly.

Improvements:

- shell list opens a scrollable shell history dialog when entries exceed toast size
- shell show displays head/tail/truncation metadata clearly
- shell ask/include actions are visible in shell detail footer
- killed/timed out/failed-to-start statuses are visually distinct
- rerun and kill feedback uses consistent language
- promoted state is clear: `promoted: yes/no`

Potential commands to document in help:

- `!cmd` run shell command
- `/shell-list`
- `/shell-show <id>`
- `/shell-include <id> [mode]`
- `/shell-ask <id> <question>`
- `/shell-rerun <id>`
- `/shell-kill <id>`

### Acceptance Criteria

- Shell history/detail surfaces are readable for repeated development loops.
- Shell failure/killed/timed-out state is obvious in list and detail views.
- Help documents shell commands without overloading insert-mode shortcuts.

## Workstream 6: Background Activity Visibility

### Problem

Async work now happens in the background. Users need enough visibility to understand whether codegg is idle, loading, or waiting.

### Design

If Phase 7 task registry exists, use it. Otherwise use existing per-dialog in-flight flags.

Show compact indicators:

- session reload loading
- import preview loading
- research loading
- memory operation loading
- file diff pending count
- shell running count
- security review running
- task/core operation pending

Surfaces:

- status bar activity chips
- sidebar small activity section if available
- `/tui-stats`
- dialog-local loading labels

### Acceptance Criteria

- Long-running async operations are visible somewhere without opening logs.
- Pending file diff count and shell running count are visible or discoverable.
- Dialog-local loading states are consistent.

## Workstream 7: Error Language and Recovery Actions

### Problem

Errors should tell the user what happened, whether it is recoverable, and what action is available.

### Standard form

Short toast:

```text
Failed to load sessions: core unavailable
```

Dialog detail:

```text
Operation: Load sessions
Status: Failed
Reason: core unavailable
Recovery: retry with r, or check daemon status with /doctor
```

Use this pattern for:

- core unavailable
- provider/model errors
- import failure
- session mutation failure
- shell failed-to-start
- render component fallback
- remote protocol unsupported message

### Acceptance Criteria

- Common failure surfaces use action-oriented text.
- Render fallback surfaces mention logs or retry only if retry exists.
- Errors avoid raw debug dumps unless in details view.

## Workstream 8: Documentation and Onboarding Polish

Update:

- `architecture/tui.md`
- `architecture/human_shell.md`
- `.opencode/skills/tui/SKILL.md`
- troubleshooting docs
- command/help docs if present

Document:

- insert vs normal mode behavior
- shell command workflow
- `/tui-stats`
- background loading behavior
- degraded render fallback behavior
- remote TUI protocol direction if Phase 8 landed

## Testing Plan

Headless render tests:

- info dialog with long output
- shell history/detail dialog
- status bar with background activity states
- dialog footer hints
- tiny terminal with active dialog and toasts

Unit tests:

- status summary priority order
- label formatting helpers
- dialog footer hint generation
- toast-vs-dialog routing helper if introduced

Manual smoke tests:

1. Run shell success/failure/killed command and inspect list/show/ask.
2. Trigger session reload and import preview; verify loading state visible.
3. Run `/tui-stats`; verify output is readable.
4. Open major dialogs and verify footer hints match keys.
5. Resize terminal to tiny/small sizes.
6. Verify help text matches insert/normal/dialog behavior.

## Acceptance Criteria

- Long structured output uses scrollable surfaces instead of long toasts.
- Status bar state is composed consistently and reflects major active operations.
- User-facing labels are consistent across TUI surfaces.
- Dialog footers match actual key handling.
- Shell workflow is discoverable and readable.
- Background work is visible or discoverable.
- Docs reflect current behavior.
- Workspace checks pass.

## Out of Scope

- Full visual redesign.
- Theme overhaul.
- Mouse-first UX.
- GUI/mobile frontend UX.
- Internationalization.
