# Project Picker Rework Milestone 002 — Help-style picker with Normal-mode filter/select plus cwd and project surfaces

Status: ready for handoff

Repository baseline: `292e5f5b` (M001 implementation commit).

Source roadmap:

- `plans/subsystems/project-picker-rework-roadmap.md#7-milestones`
  (Milestone 2)

Long-term requirements:

- `plans/000-long-term-specification.md#17-tui-target-architecture`
- `plans/001-terminology-and-domain-model.md`

Applicable ADRs:

- None required (no protocol, storage, identity, or authority change).

Primary class: capability

## 1. Objective

Rework the project picker into the `/help` dialog format (title /
filter row / rows / footer plus `NOR`/`INS` badge) with filtering and
selection fully usable in Normal mode, and surface the active project
name plus cwd on the status line and project names in the sessions
panel — with no protocol or storage change.

## 2. Why this milestone is ready

Blocked, not ready: M1 must close first (shared key/help/description
surface; M1 removes the `Ctrl+\` chord this milestone's help copy and
tests assume gone). No other hard dependency; the `/help` render and key
pattern to borrow is stable.

## 3. Current implementation evidence

- Picker state `ProjectPickerState` with `PickerPhase::{Catalog,
  WorkspaceSelection, RegistrationInput, RegistrationConfirm, Error}`
  (`src/tui/app/state/project_picker.rs`); render
  `render_picker_body` in `src/tui/components/dialogs/project_picker.rs`
  (custom search header, `Enter select | Esc close | Ctrl+R register`
  footer, no mode badge); key handling `handle_project_picker_key` and
  phase handlers in `src/tui/app/mod.rs` (`j/k` only when query empty).
- `/help` pattern: `HelpDialog` (`src/tui/components/dialogs/help.rs`)
  renders 4-chunk layout with filter row (`search {query}_
  {matches}/{total}`), `INS`/`NOR` badge, scrollable rows, mode footer;
  `handle_key` scrolls in Normal, types in Insert, `Esc` steps
  Insert→Normal→clear→close.
- Status line `StatusBarWidget::render`
  (`src/tui/components/status_bar.rs`, summary built
  `src/tui/app/render.rs:1686`) shows status/thinking/tokens — no
  cwd/project. Per-tab root available via
  `ProjectTabState.workspace_root`
  (`src/tui/app/state/project_tabs.rs`) and `active_workspace_root()` /
  `project_execution_context()` (`src/tui/app/project_session.rs`).
- Sessions panel `SessionDialog`
  (`src/tui/components/dialogs/session.rs`) rows are daemon `Session`
  records only (no project name); per-row project display available via
  tab `project_details` / `SessionSummaryCacheEntry`.

## 4. Invariants that must not regress

- Insert-mode typing preserved everywhere: text-modifier letters type
  text, custom bindings win (existing precedence tests).
- Picker generation/request scoping: stale catalog/detail completions
  cannot mutate the wrong tab (inherited guards).
- Picker caps (`MAX_PROJECT_LIST_ITEMS`, visible rows, tab limit) kept.
- `Shift+F` Normal-only, `/projects` mid-prompt, `\` vim-Normal unchanged
  (M1 outcome preserved).
- Daemon cwd untouched; no new `current_dir()` (guard stays clean).

## 5. Scope

### In scope

- Picker dialog render in `/help` format with mode badge and footer.
- Normal-mode filter + select (`j/k` move with non-empty query, typing
  filters without leaving Normal, `Enter` commits, `Esc` ladder,
  `Tab` archived toggle, `Ctrl+R` register path preserved).
- Inactive (archived) projects sink to the picker list bottom.
- Status line: active project name + cwd/worktree path (middle-truncated,
  bounded width).
- Sessions panel: per-row project name (or active-project header if
  per-row join proves disproportionate — decide in-package, record in
  closure).

### Explicitly out of scope

- Row mutations `d`/`a`/`e` and destroy (M3/M4).
- Identity, auto-create/load, cwd switching (M5).
- Any protocol, storage, or authz change.
- Split panels / split-switch chords (deferred).

## 6. Required production changes

### Core/domain

None.

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

None; catalog/detail async flow and generation guards unchanged.

### Frontend or operator surface

- Rework `ProjectPickerDialog` render to title / filter row / rows /
  footer + `NOR`/`INS` badge, mirroring `HelpDialog::render_into`.
- Rework picker key handling so Normal mode filters and selects:
  `Char` input appends to query without mode switch (or via explicit
  `/`/`i` entry to Insert — pick one, document it, pin with tests);
  `j/k` navigate regardless of query; `Enter` commits selection;
  `Esc` follows Insert→Normal→clear→close.
- Sort inactive to the bottom in `filtered_indices` (stable within
  groups, score order otherwise preserved).
- Status bar: add bounded project-name + cwd segment sourced from the
  active tab's `ExecutionContext`.
- Sessions panel: add project-name column/header sourced from tab state.

### Security and authorization

None.

### Documentation and static guards

- `docs/tui.md` picker section: new keys, badge, footer, status-line
  field, sessions-panel column.
- No new guard; keep documented-keys-resolve and help-audit suites green.

## 7. Ordered work packages

### Work package A — Help-style picker render

Intent: the picker looks and reads like `/help`.

Required changes: dialog layout, filter row with match counts, mode
badge, footer hints for the new key contract.

Acceptance evidence: render tests pinning layout rows, badge, footer.

### Work package B — Normal-mode filter/select keys

Intent: full picker operation without leaving Normal mode.

Required changes: key-routing rework per §6; Insert path (type to
filter, `Esc` to Normal) preserved.

Acceptance evidence: key-handling tests for move/filter/commit/clear/
close in both modes; archived-toggle and register-entry still reachable.

### Work package C — Status line and sessions panel surfaces

Intent: cwd and project context always visible.

Required changes: bounded status segments; project-name column/header.

Acceptance evidence: tests asserting active-tab values render (and
truncate) correctly; empty/no-context states render sanely.

## 8. Failure, cancellation, restart, and contention semantics

Stale catalog/detail completions still drop on generation mismatch. No
durable writes; no new failure modes. Empty catalog and unsupported
transport states render as today.

## 9. Compatibility and migration

Key-contract change is TUI-local; custom user bindings keep precedence.
No config, protocol, or storage migration.

## 10. Required tests

### Focused unit tests

- Picker key tests: Normal filter+select/commit/clear/close;
  Insert type/`Esc`/commit; `j/k` with non-empty query; archived toggle;
  register entry.
- Render tests: badge, filter counts, footer, inactive-at-bottom order.
- Status/sessions surface tests: values, truncation, empty states.

### Integration tests

- `modifier_chords` suite; documented-keys-resolve audit; help-audit
  suites (shared key surface).

### Restart and recovery tests

None (no durable state).

### Contention and cancellation tests

- Stale-generation completion still dropped (existing guards; add a
  regression test if uncovered for the new dialog paths).

### Security and negative tests

None (no authority change).

### Migration and compatibility tests

None.

## 11. Required verification commands

```bash
cargo test -p codegg --lib tui::
cargo test -p codegg --test modifier_chords
scripts/verify.sh quick
```

Do not claim commands that were not actually run in the closure record.

## 12. Documentation updates

- `docs/tui.md`: picker keys/badge/footer, status-line cwd field,
  sessions-panel project column.
- `architecture/tui.md` if it pins the old picker interaction contract.

## 13. Acceptance criteria

- Picker filters and commits entirely from Normal mode; Insert path
  unchanged and documented.
- Inactive projects list last.
- Status line shows active project name + cwd; sessions panel shows
  project names.
- Focused suites + quick verification pass.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- M1 has not strictly closed (chord/help surface differs from §3);
- the baseline lacks the `Shift+F` + `/projects` surface;
- the sessions-panel per-row join requires daemon/protocol change
  (that belongs to M3+, not here);
- scope would expand into mutations or identity work.

## 15. Closure evidence required

- Focused + integration suite results and quick verification outcome
  (local vs CI labeled truthfully).
- Manual note after reinstall + daemon restart: Normal-only
  filter/select flow, badge/footer, status cwd, panel project names.
- Unblock audit: M3/M4 interface readiness confirmed or qualified.

## 16. Handoff notes

- Rebase onto the M1 closure head; do not implement against a tree
  where `Ctrl+\` still exists.
- Keep row-action hooks (selection context per row) clean for M3/M4;
  do not pre-build the actions.
- M3/M4 plans exist but are blocked on the M2 interface stabilizing —
  avoid key choices (`d`/`a`/`e`) that collide with the new picker
  contract; reserve them.
