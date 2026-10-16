# Project Picker Rework Milestone 002 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/project-picker-rework/002-help-style-picker-and-surfaces.md`
Source subsystem roadmap: `plans/subsystems/project-picker-rework-roadmap.md#7-milestones`
Repository baseline reviewed: `1f95c1f1`
Implementation commit: `94300835` — help-style project picker, Normal-mode filtering/selection, and project context surfaces

## 1. Executive finding

M002 is strictly closed. The picker now follows the `/help` layout and
supports filter, movement, selection, archive visibility, and the
Insert/Normal Escape ladder. The active project and working root appear
on the status line; the sessions dialog shows the active project's
context. No protocol, storage, or daemon-cwd behavior changed. Focused
TUI suites, modifier-chord integration, and repository quick
verification passed locally.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Finding |
|---|---|---|
| Help-style picker with `NOR`/`INS`, filter count, and footer | `tui::components::dialogs::project_picker::` render tests; full TUI library suite | Satisfied |
| Normal-mode query, movement, select, archive toggle, and Escape ladder | `tui::app::state::project_picker::` tests; full TUI library suite | Satisfied |
| Inactive projects sort after active projects | Picker ordering test; full TUI library suite | Satisfied |
| Active project and root are visible with bounded truncation | Status-bar tests; session-dialog context render test | Satisfied |
| Shared key contract remains valid | `cargo test -p codegg --test modifier_chords`: 16 passed | Satisfied |
| Workspace boundaries and sanity | `scripts/verify.sh quick`: passed, including `cargo check --workspace --all-targets --locked` | Satisfied |

## 3. Production implementation evidence

- Reworked picker state and key handling so Normal mode filters and
  navigates without mode changes; Insert mode remains available, and
  Enter, Tab, and the Escape ladder have explicit state transitions.
- Reworked the dialog into title, filter/count, rows, and footer areas
  with a `NOR`/`INS` badge and mode-specific hints.
- Sorted active projects ahead of archived projects.
- Added bounded active-project/root context to the status bar and an
  active-project heading to the sessions dialog.
- Updated `docs/tui.md` and `architecture/tui.md` with the interaction
  and context surfaces.
- Kept generation scoping, picker caps, and daemon cwd ownership intact.

## 4. Verification executed

All commands ran locally on implementation commit `94300835`. The
machine's default pkg-config search selected arm64 MacPorts native
libraries while the active Rust target was x86_64, so Rust build/test
commands used `PKG_CONFIG_LIBDIR=/usr/local/lib/pkgconfig` and
`CARGO_BUILD_JOBS=2`.

- `cargo test -p codegg --lib tui::` — 1,706 passed; 4,491 filtered out.
- `cargo test -p codegg --test modifier_chords` — 16 passed.
- `scripts/verify.sh quick` — passed, including format, repository
  guards, generated-agent check, and workspace all-target check.
- `git diff --check` — passed before the implementation commit.
- Hosted CI and interactive reinstall/daemon-restart checks were not run.

The broad TUI suite also found one stale dashboard key-collision
expectation from the retired Ctrl+\\ opener. That assertion was updated
to expect no picker dispatch, then the full TUI library suite passed.
This was a stale test expectation, not a production regression; M001's
original focused verification remains accurately recorded in its
closure.

## 5. Interface and invariant review

The status bar obtains the active tab's root through its execution
context. The sessions surface uses the active-project heading allowed
by the plan where a per-row join would require broader data plumbing.
Picker generation guards, visible-row and catalog caps, user binding
precedence, daemon cwd ownership, and the `Shift+F`, `/projects`, and
vim-Normal `\\` routes remain intact.

## 6. Failure, recovery, and compatibility review

M002 adds no durable writes, migrations, protocol operations, or new
recovery paths. Existing asynchronous catalog/detail generation guards
remain in place. The key-contract update is TUI-local and custom binding
precedence is preserved.

## 7. Security review

No authority or filesystem boundary changed. The project root shown in
the status surface is read from the active tab's existing execution
context. No daemon process cwd mutation was introduced.

## 8. Unresolved findings

None within M002 scope. Hosted CI and interactive restart checks were
not run; local acceptance suites and quick verification passed.

## 9. Downstream dependency audit

M002 closes the stable picker-interface dependency for both M003 and
M004. The accepted ADR-0033 remains satisfied, so both plans move from
blocked to ready:

- M003 lifecycle and confirmed destroy is ready and is next in the
  requested sequential order.
- M004 rename and repath is also ready and can follow M003; its
  interface requirements are independent after M002.
- M005 remains blocked on strict closure of M2–M4.
- M006 multi-cwd splits remain deferred and are not registered as
  implementation work.

The project-picker roadmap remains active while M003–M005 are open.

## 10. Registry updates

- M002 moved to closed and this closure was added to recently closed
  work.
- M003 and M004 moved to ready and the dependency-ready table.
- M005 remains blocked on M2–M4; its status is unchanged.
