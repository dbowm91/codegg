# Command Surface Reconciliation M001 — TUI Command Action Convergence

Status: ready

Corrective roadmap:
plans/subsystems/command-surface-reconciliation-corrective-addendum.md

Historical implementation:
plans/implementation/tui-project-sessions/010-command-discovery-keybinding-convergence.md

Historical closure:
plans/closure/tui-project-sessions/010-status.md

Repository baseline reviewed: 5af6766a04d6326ba1e7bb3695b2f0422399e9ee

## 1. Objective

Make the canonical TUI command registry authoritative for both discovery and
execution. Remove the current class of defects where a string branch exists but cannot
be reached because registry resolution rejects it first, or a registry entry has no
action.

## 2. Why M010 did not catch this

M010's scope was discovery metadata/keybinding convergence. Its closure explicitly
records that the legacy command strings, parser and dispatcher remained intact. The
coverage tests asserted metadata/collision behavior for registered commands, not a
bijection between registered command entries and executable dispatcher branches.

This corrective milestone retains all accepted M010 metadata/project-scope behavior
and adds the missing execution invariant.

## 3. Typed action model

Extend the built-in command definition with a typed action rather than relying on its
name being matched again later.

A suitable shape is conceptually:

- `CommandAction::Dialog(DialogType)`
- `CommandAction::Builtin(BuiltinSlashAction)`
- `CommandAction::Template(...)`
- `CommandAction::Process(...)`
- any existing dynamic/project/plugin action adapter needed to preserve M010 scope.

Use an exhaustive `BuiltinSlashAction` enum for built-in operations that currently
live in `execute_command` string arms. The registry may still own names, aliases,
domain/scope/source/keywords and display metadata, but dispatch receives the resolved
action, not raw text that must be reclassified.

Do not merge arbitrary key actions and slash actions into one enum; M010 explicitly
kept those concepts separate.

## 4. Parsing and dispatch

The input path should be:

raw slash text → registry name/alias resolution → parsed args → typed command action →
existing domain handler/TuiCommand/dialog/template/process path.

The built-in dispatcher may remain a function, but it must match
`BuiltinSlashAction` exhaustively. No production branch should compare a raw command
name to introduce a new built-in behavior.

Aliases resolve to the same typed action as their canonical command.

Dynamic command files continue through their existing template/process execution path
and collision rules.

## 5. Reconcile current drift

At implementation start re-run the automated registry/dispatcher census, then at
minimum resolve the audit findings:

- `/model`: preserve the existing model-dialog behavior as a canonical command or
  alias consistent with `/models`.
- `/checkpoints` and `/history`: retain only as documented aliases if they are
  intended to reach the current edit-checkpoint list; do not let ambiguous “history”
  shadow unrelated session history.
- `/edit-undo` and `/edit-reapply`: register the existing durable edit-history
  actions so the closed Runtime Safety edit-history capability is actually reachable.
- `/tool-contracts`: register the existing tool-contract operation.
- `/worktree`: register the existing worktree operation/fallback.
- `/checkpoint`: trace the historical/current intended handler. If there is a
  current session-checkpoint operation with the semantics promised by its description,
  wire that typed action and test it. If no such user operation exists, remove it from
  the canonical discoverable registry and retain only an evidence-backed compatibility
  alias if it maps to a real current command. Do not silently redirect it to goal or
  edit checkpoints unless that is demonstrably the intended compatibility behavior.

Treat the list as the initial regression set, not the complete census.

## 6. Generated/guarded documentation

Make `architecture/command.md` command tables/counts consume or validate against
canonical registry metadata. A small generation/verification helper is acceptable;
avoid hand-maintaining a hard-coded count in several documents.

Preserve narrative architecture docs separately from generated tabular metadata.

Update CHANGELOG only if repository policy requires it for corrective command
availability.

## 7. Tests

Add exhaustive structural tests:

- every built-in registry entry has one action;
- every `BuiltinSlashAction` is referenced by at least one canonical command;
- alias names are unique and resolve to the canonical action;
- no built-in aliases collide with dynamic reserved names under existing precedence
  rules;
- dialog/template/process actions remain executable without entering the built-in
  action switch;
- the seven known previously unreachable literals resolve and execute the expected
  bounded action;
- `/checkpoint` matches its chosen evidence-backed disposition;
- observer/read-only restrictions are still enforced after typed dispatch;
- command palette/project scope metadata from M010 is unchanged.

Where possible remove the static raw-name comparison script after typed exhaustiveness
makes it impossible by construction.

## 8. Compatibility

Do not rename public commands merely to reduce visual clutter. If future grouping such
as `/lsp ...` or `/terminal ...` is desired, treat the old names as hidden aliases
and plan that UX separately.

This milestone should reduce implementation duplication without broad behavioral
change.

## 9. Verification

- focused `tui::command` registry tests
- slash-command parsing/dispatch tests
- command palette/project switching tests from M010
- observer/read-only command tests
- edit checkpoint TUI tests
- worktree/tool-contract focused tests
- `cargo fmt --all -- --check`
- strict workspace Clippy
- `scripts/verify.sh quick`
- `git diff --check`

## 10. Acceptance

Close only when adding a new built-in executable command requires touching one typed
registry/action authority and the compiler/tests make a registry-only or
dispatcher-only command impossible to land unnoticed.

The closure record must include the final disposition of every audit-listed command
and the refreshed canonical command census.
