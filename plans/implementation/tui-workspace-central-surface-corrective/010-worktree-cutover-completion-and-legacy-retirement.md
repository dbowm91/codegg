# TUI Workspace Central Surface Corrective Milestone 010 — Finish the `/worktree` cutover and retire the unreachable list path

Status: ready for handoff

Repository baseline: `256b215ac` on `feat/auto-20261126-cae46b00` (worktree clean; verification performed 2026-11-27)

Source roadmap:

- `plans/subsystems/tui-workspace-central-surface-corrective-addendum.md#13-post-closure-verification-and-corrective-milestones`
- `plans/subsystems/tui-workspace-central-surface-corrective-addendum.md#m005--worktree-navigates-into-workspace-instead-of-toast-or-dialog`

Source implementation plan:

- `plans/implementation/tui-workspace-central-surface-corrective/005-worktree-navigates-to-workspace.md` §5 (no toast and no dialog of its own) and §10 (the missing assertions).

Source closure record this milestone corrects:

- `plans/closure/tui-workspace-central-surface-corrective/005-status.md` — records M005 closed while the command's own runtime test was red and three descriptions still advertised the old behaviour.

Long-term requirements:

- `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui` — one navigation surface for projects, workspaces and worktrees.
- `plans/000-long-term-specification.md#25-tui-target-behavior` — commands describe what they do; the help surface is part of the contract.

Applicable ADRs:

- `plans/adrs/ADR-0052-cross-frontend-command-execution-ownership.md` — the catalog owns the command list and its description; a frontend row must match it.
- `plans/adrs/ADR-0033-directory-tied-project-identity-and-client-working-directory.md` — a selector resolves against the catalog, not against the process cwd.

Primary class: **capability** (the command navigates) plus **polish** (the unreachable surface and its documents are removed rather than left to rot).

## 1. Objective

`/worktree [selector]` is a complete, tested navigation command — no toast, no dialog, the named target revealed — its description says so in both the catalog and the TUI, and the TUI's unreachable `WorktreeList` surface is gone.

## 2. Why this milestone is ready

- The selector is implemented and bounded: the argument is trimmed, an empty argument returns, and more than **256** characters is rejected (`crates/codegg-tui/src/commands/workspace_view.rs:1247-1265`); resolution is exact and case-insensitive over workspace id/name and worktree id/branch (`:1277-1279`, `:1394-1458`); a miss becomes `view.notice` (`:1467-1473`), which the dashboard paints (`crates/codegg-tui/src/components/dialogs/workspace_dashboard.rs:309-313`).
- Live receipts already show all three spellings behaving: bare `/worktree` lands on the route with no toast and no dialog; `/worktree repo` lands and reveals the workspace (the membership list then loads); `/worktree nosuchtarget` stays on the route with `No workspace or worktree matched /worktree nosuchtarget`.
- The TUI list path has **no sender left**. `dispatch_worktree` was deleted; the only remaining occurrence of `TuiCommand::WorktreeList` outside the dead handler is the dispatch arm itself (`crates/codegg-tui/src/runtime/command_dispatch.rs:814`).
- The command descriptions are exactly compared by a guard, so the three sites must move together: `scripts/check_command_catalog_ownership.py:518-527` reports a mismatch when a canonical row's description differs from the catalog's.

## 3. Current implementation evidence

### 3.1 The red test

`nested_runtime_command_dispatch::worktree_command_completes_inside_the_event_loop_runtime` (`crates/codegg-tui/tests/tui_render/nested_runtime_command_dispatch.inc:119-141`) asserts the toast `No worktrees found` and that command mode exited. It **passes at `eefbfa0fa` and fails at `256b215ac`** — measured with the same binary tree (baseline exported via `git archive`, shared target dir, identical environment). It is the only failure this line introduced; the other eight were red before it.

The family exists to prove a command does not block the event loop, so the test's *purpose* survives the cutover; only its assertion is obsolete.

### 3.2 The unreachable surface

| Site | What it is |
|---|---|
| `crates/codegg-tui/src/app/types.rs:45`, `:135` | `TuiCommand::WorktreeList` variant and its classification |
| `crates/codegg-tui/src/app/commands.rs:274`, `:972-977` | the message-side variants (`WorktreeListed`) |
| `crates/codegg-tui/src/runtime/command_dispatch.rs:814`, `:1582` | the dispatch arm and the completion arm |
| `crates/codegg-tui/src/commands/tasks.rs:558-650` | the handler that builds and sends the request |
| `crates/codegg-tui/src/components/component.rs:56`, `:130`, `:192`, `:399` | `DialogType::WorktreeList` plumbing |
| `crates/codegg-tui/src/components/dialogs/info.rs:29-48`, `:108-149` | the dialog body and its title |
| `crates/codegg-tui/src/app/state/dialog.rs:152-153` | the dialog state field |
| `crates/codegg-tui/src/app/render.rs:3208` | the render arm |
| `crates/codegg-tui/tests/tui_render/tui_render.inc:655`, `:1625`; `tui_info_dialog_panel_chrome.inc:691-710` | tests of the dead surface |
| `crates/codegg-tui/tests/tui_render/invariants/fixtures.rs:438-440`; `invariants/vocabulary.rs:361` | the recorded hint row `WorktreeList — j/k scroll` |

**Keep** `CoreRequest::WorktreeList` (`src/core/daemon_projects.rs:77`, `src/core/daemon_family.rs:292`), `GitOperation::WorktreeList` (`src/git_service.rs:346`, `:386`, `:1128`, `:1944`) and `src/command_intent/plan.rs:326`. They are the daemon and Git layers, not the TUI list.

### 3.3 Stale descriptions and prose

- `crates/codegg-tui/src/command.rs:987` — `"List worktrees for the active workspace (/worktree)"`; `Command::with_description` also derives search keywords from it (`:375-378`).
- `crates/codegg-core/src/command_catalog.rs:3131` — the same string, `kind: CommandKind::FrontendEffect`.
- `architecture/command.md:393` — `List worktrees for the active workspace`.
- `architecture/tui.md:264-280` still describes the old nested-`block_on` behaviour; `:5632-5634`, `:5668`, `:5694`, `:8359-8360` describe the old list; `architecture/worktree.md:233-235` and `docs/tui.md:151-154` need the navigable spelling.

## 4. Invariants that must not regress

- The command never blocks the event loop and never leaves command mode set.
- No toast and no dialog originate from `/worktree` (`005` plan §5).
- A selector resolves against the checked-in workspace/worktree identities only — never against the process cwd.
- The 256-character bound and the empty-argument no-op stay.
- One description, two spellings: the catalog and the TUI row must be byte-identical; the guard is the proof.
- **The eight `tui_render` failures present at the baseline are pre-existing and out of scope.** After this milestone the family is expected to be red for exactly those eight; the ninth (this milestone's) must be gone. Do not repair the advertised-keys residue (`TaskView h`/`l`) or the docking failures here.

## 5. Scope

### In scope

- Rewrite the runtime-family test to assert navigation, no toast, no dialog, and command-mode exit.
- Selector coverage: blank, workspace id, workspace name, worktree branch, unknown, and the 256-character boundary.
- Delete the unreachable TUI list surface listed in §3.2, with its tests and its recorded hint row.
- Update the three descriptions in one change.
- Update the stale architecture and user documentation.

### Explicitly out of scope

- `CoreRequest::WorktreeList`, `GitOperation::WorktreeList`, `command_intent` and every daemon/Git consumer.
- A new selector grammar (`project:`/`workspace:` prefixes) — matching stays exact and case-insensitive.
- Any change to the workspace route's other keys.
- The eight pre-existing failures, including the advertised-keys baseline's `TaskView` residue.
- A dedicated 256-character test is **optional** (the original plan did not require one); it is included below as a recommended, cheap pin.

## 6. Required production changes

### Frontend

1. Delete `TuiCommand::WorktreeList`/`WorktreeListed`, their dispatch and completion arms, the `tasks.rs` handler, the dialog type and state field, the render arm, and the tests that only exercised them. Delete in one change so no half-wired variant survives.
2. Remove the recorded `WorktreeList` hint row from `invariants/vocabulary.rs` and its fixture entry, and re-bank only if the measurement moves.

### Documentation and static guards

Descriptions: `command.rs:987` and `command_catalog.rs:3131` must carry the same new text; propose `Open a workspace or worktree in the workspace view (/worktree)`. `architecture/command.md:393` follows. Then `architecture/tui.md` (`:264-280` and the four list references), `architecture/worktree.md:233-235`, `docs/tui.md:151-154`.

Guards to run: `python3 scripts/check_command_catalog_ownership.py` (the exact-text rule), `python3 scripts/check_frontend_action_set.py`, `python3 scripts/check_tui_transport_authority.py`.

## 7. Ordered work packages

### Work package A — Make the red test assert the new contract

Intent: the cutover's own regression is green and meaningful.

Required changes: rename to `worktree_command_routes_without_waiting_on_a_runtime_handle`; assert the dashboard is open, the selector resolved (the target is revealed or the notice names the miss), `toast_messages` is empty, no picker/dialog is mounted, and `command_mode` is false. Keep the runtime construction so the non-blocking claim is still proven.

Acceptance evidence: fails on `256b215ac` for the right reason (a navigation assertion, not a panic) and passes after.

**Delivered correction to this bar.** The rewritten test does **not** fail on `256b215ac`, and cannot: `/worktree <selector>` already navigated at that commit — `dispatch_worktree` (`commands/workspace_dashboard.rs:992`) and `begin_selector` (`commands/workspace_view.rs:1247`) are both there, and only the old assertion (the removed `No worktrees found` toast) was red. The test was therefore written to pin the contract going forward rather than to reproduce a red; §3.1's measurement stands and is a different claim from this one. Recorded in `plans/closure/tui-workspace-central-surface-corrective/010-status.md`.

### Work package B — Selector coverage

Intent: every spelling is pinned.

Required changes: unit tests beside the selector (`commands/workspace_dashboard.rs` tests): blank, workspace id, workspace name, worktree branch, unknown; plus the 256/257 boundary (recommended).

Acceptance evidence: each asserts the resolved identity or the notice text, not merely that no panic occurred.

### Work package C — Retire the dead surface

Intent: no second, unreachable list path.

Required changes: the deletions in §3.2; the hint-row removal.

Acceptance evidence: `grep -rn 'WorktreeList' crates/codegg-tui/src` returns only the workspace-view types that mean managed worktrees (if any), and the family run adds no failure.

### Work package D — One description, three documents

Intent: the help surface stops lying.

Required changes: the two code sites together, then the docs.

Acceptance evidence: `check_command_catalog_ownership.py` clean; the help overlay and `/help` render the new text.

## 8. Failure, cancellation, restart, and contention semantics

An unknown selector is a notice, not an error state, and the route stays usable. A selector longer than the bound is refused before any lookup. Rapid repeat invocations replace the pending selector rather than queueing. Nothing is persisted, so restart semantics are unchanged.

## 9. Compatibility and migration

The command keeps its name and its frontend-effect kind. A user who relied on the toast loses nothing: it was the command's only output and the route is a superset. No protocol or storage change; the daemon's `WorktreeList` request remains available to other callers.

## 10. Required tests

1. `worktree_command_routes_without_waiting_on_a_runtime_handle` (rewritten in place).
2. Selector unit tests (work package B).
3. The dead-surface tests are deleted, not skipped.

### Focused unit tests

`cargo test -p codegg-tui --lib workspace_dashboard`

### Integration tests

`cargo test -p codegg-tui --test tui_render` — unfiltered. Expected result: the eight pre-existing failures and nothing else.

## 11. Required verification commands

```bash
cargo test -p codegg-tui --lib workspace_dashboard
cargo test -p codegg-tui --test tui_render          # unfiltered; exactly the eight known reds
python3 scripts/check_command_catalog_ownership.py
scripts/verify.sh quick
scripts/verify.sh scoped 256b215ac
```

Live receipt (addendum §13.2): `/worktree`, `/worktree repo`, `/worktree nosuchtarget` — capture the route frame, the reveal, and the notice; assert no toast row and no dialog in each.

## 12. Documentation updates

`architecture/command.md`, `architecture/tui.md` (five sites), `architecture/worktree.md`, `docs/tui.md`, and the two code descriptions.

## 13. Acceptance criteria

- The rewritten runtime test passes and asserts navigation, emptiness of toasts, absence of a dialog, and command-mode exit.
- No sender or consumer of the deleted TUI list path remains; the daemon and Git request families are untouched.
- Both descriptions are identical and the ownership guard is clean.
- The unfiltered family run shows exactly the eight pre-existing failures.
- The three live frames are captured.

## 14. Stop conditions

- Stop if deleting the hint row changes the vocabulary measurement beyond the `WorktreeList` row, or if the family's pre-existing failure set grows: record the measurement and report instead of widening into the advertised-keys line.
- Stop if any consumer of the TUI `WorktreeList` variant turns out to be reachable; report the path rather than deleting it.
- Do not touch `CoreRequest::WorktreeList` or `GitOperation::WorktreeList`.

## 15. Closure evidence required

`plans/closure/tui-workspace-central-surface-corrective/010-status.md` with: the unfiltered family run and the named eight, the rewritten test's baseline-versus-fix result, the deletion inventory, the guard output, and the three receipt paths.

## 16. Handoff notes

- The baseline-versus-HEAD measurement is the proof that this test is a regression, not an environment effect: `git archive eefbfa0fa | tar -x -C <tmp>` and run the same filter with a shared `CARGO_TARGET_DIR`. Record both results in the closure.
- Do the deletions after the test rewrite so the family tells you immediately if a removal broke something reachable.
- The 256-character bound is at `workspace_view.rs:1247-1265`; do not re-implement it.
