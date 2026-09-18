# Command Surface Reconciliation M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/command-surface-reconciliation-corrective/001-tui-command-action-convergence.md`

Source subsystem roadmap:

- `plans/subsystems/command-surface-reconciliation-corrective-addendum.md#5-milestones`

Repository baseline reviewed: `6e5203b6`

Implementation commits or pull requests:

- `6e5203b6` — converge TUI commands onto typed action authority

## 1. Executive finding

M001 is complete. The canonical TUI command registry is now the single
executable authority for slash discovery and dispatch. Every built-in
registry entry carries one typed `CommandAction`; the dispatcher matches
the exhaustive `BuiltinSlashAction` enum and no production branch
introduces built-in behavior by comparing a raw command name. All seven
audit-listed unreachable literals resolve, `/checkpoint` has an
evidence-backed removal disposition, and the previously dead
`/shell-ask` registration is wired. The refreshed canonical census is
149 built-in commands (142 built-in actions, 5 dialog actions, 2 template
actions).

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Typed action model (`Dialog`/`Builtin`/`Template`/`Process`/plugin adapter; exhaustive `BuiltinSlashAction`; key/ slash actions not merged) | `CommandAction` + 142-variant `BuiltinSlashAction` in `src/tui/command.rs`; `Command::new` takes `CommandAction`; `dispatch_builtin_command` matches `B::*` exhaustively | pass |
| Input path raw text → resolution → parsed args → typed action → domain handler | `handle_slash_command` resolves via `find_by_name_or_alias`, syncs palette query, calls `execute_command(&cmd, ...)`; `execute_command` matches `cmd.action` | pass |
| Aliases resolve to canonical action | `aliases_resolve_to_canonical_action` test; `/model`→`/models`, `/checkpoints`+`/history`→`/edit-checkpoints` | pass |
| Dynamic files keep template/process path + collision rules | `from_dynamic_command` sets `Template`/`Process`; `builtin_aliases_win_dynamic_collisions`, `project_catalog_order_is_stable_and_builtin_wins_collisions` | pass |
| `/model` preserves model-dialog behavior consistent with `/models` | `/model` registered as alias of `/models` (`Dialog(Model)`); `/model` dispatcher arm removed; `previously_unreachable_literals_now_resolve_to_bounded_actions` | pass |
| `/checkpoints` + `/history` documented aliases only; no shadowing | Canonical `/edit-checkpoints` with aliases `/checkpoints`, `/history` (M012 closure evidence); no registered `/history` canonical; `checkpoint_has_evidence_backed_removal_disposition` | pass |
| `/edit-undo` + `/edit-reapply` registered (durable edit-history reachable) | Canonical entries with `Builtin(EditUndo/EditReapply)`; existing `TuiCommand::EditUndo*` handlers reached | pass |
| `/tool-contracts` registered | Canonical entry with `Builtin(ToolContracts)`; existing `TuiCommand::ToolContracts` handler reached | pass |
| `/worktree` registered | Canonical entry with `Builtin(Worktree)`; existing `WorktreeList` handler reached | pass |
| `/checkpoint` evidence-backed disposition | Removed from registry (registered since `9bf9293b` with zero execution branches; no session-checkpoint op with promised semantics; goal checkpoints goal-scoped, edit checkpoints via `/edit-checkpoints`); `checkpoint_has_evidence_backed_removal_disposition`; observer dead reference replaced | pass |
| `architecture/command.md` consumes/validates canonical metadata | Counts 142→149 refreshed; typed-action section added; stale `/git-status`, `/provider-connections` rows removed; `command_docs_count_matches_registry` test parses the doc and fails on drift | pass |
| Exhaustive structural tests | 18 `tui::command::tests` (bijection, coherence, alias, precedence, bypass, audit literals, checkpoint, observer) | pass |
| Observer/read-only restrictions enforced | `reconciled_commands_stay_observer_blocked` + updated `read_only_policy_blocks_every_control_family` (fail-closed allowlist; new commands blocked) | pass |
| M010 palette/project-scope metadata unchanged | `builtins_have_discovery_metadata`, palette/project catalog tests, 936 `tui::` lib tests, 165 `tui` integration tests | pass |
| Static raw-name comparison removed | No `match cmd.name.as_str()` remains; only arg-parsing `strip_prefix`/`trim_start_matches` inside arms plus observer fallback name | pass |

### Final disposition of every audit-listed command

| Literal | Disposition | Evidence |
|---|---|---|
| `/model` | Documented alias of canonical `/models` (`Dialog(Model)`) | Registry alias; arm removed; test resolves to `/models` |
| `/checkpoints` | Documented alias of canonical `/edit-checkpoints` (`Builtin(EditCheckpoints)`) | Registry alias; M012 `/edit-checkpoints (also aliased /history)` closure text; test resolves to `/edit-checkpoints` |
| `/history` | Documented alias of canonical `/edit-checkpoints` | Same as above; no session-history shadowing (no canonical `/history` exists) |
| `/edit-undo` | Canonical (`Builtin(EditUndo)`) | New registry entry; pre-existing `EditUndoLatest`/`EditUndo` handler |
| `/edit-reapply` | Canonical (`Builtin(EditReapply)`) | New registry entry; pre-existing `EditReapplyLatest`/`EditReapply` handler |
| `/tool-contracts` | Canonical (`Builtin(ToolContracts)`) | New registry entry; pre-existing `TuiCommand::ToolContracts` handler |
| `/worktree` | Canonical (`Builtin(Worktree)`) | New registry entry; pre-existing `WorktreeList` handler + fallback |
| `/checkpoint` | Removed, not redirected | Zero branches since introduction; no matching user operation; removal test |

### Refreshed canonical command census

149 built-in commands: 142 `Builtin` actions (see `all_builtin_variants`
in `src/tui/command.rs` for the exhaustive list), 5 `Dialog` actions
(`/models`, `/mcps`, `/keybinds`, `/stats`, `/review`), 2 `Template`
actions (`/pr`, `/issue`).

Beyond the audit list, the census also reconciled: `/shell-ask` was
registered but had no `execute_command` arm (its handler text sat dead
inside the MCP-dialog action match); it is now canonical
`Builtin(ShellAsk)` with that logic moved into the typed dispatcher and
the dead arm deleted. `/tasks`, `/task`, `/workspace`,
`/security-review-show` carried dialog fields whose generic open path is
a documented no-op; they are now `Builtin` actions reaching the correct
mounting/guarded handlers. `/mcps` and `/review` dead dispatcher arms
were removed in favor of their `Dialog` authority.

## 3. Production implementation evidence

- `src/tui/command.rs` — `BuiltinSlashAction` (142 variants),
  `CommandAction` (`Dialog`/`Builtin`/`Template`/`Process`/`Plugin`),
  `Command.action` field, `Command::new` takes the action and derives
  `dialog` from it; `built_in_commands` sets one explicit action per
  entry (149 entries); `from_dynamic_command` maps to
  `Template`/`Process`; `from_plugin_command` maps to `Plugin`;
  10 new tests plus strengthened conversion assertions.
- `src/tui/app/mod.rs` — `execute_command` matches `cmd.action`
  (`Dialog`→`open_dialog`, `Process`→`PluginCommandRun`,
  `Template`→template render, `Plugin`→bounded toast,
  `Builtin`→`dispatch_builtin_command`); new exhaustive
  `dispatch_builtin_command` (141 moved arms + `ShellAsk`); dead
  `/shell-ask` MCP-dialog arm deleted; `handle_slash_command` routes
  all slash text (including `/search`) through registry resolution and
  syncs the palette query.
- `src/tui/app/state/observe.rs` — stale `/checkpoint` block
  assertion replaced with the reconciled blocked set
  (`/worktree`, `/edit-undo`, `/edit-reapply`, `/edit-checkpoints`,
  `/checkpoints`, `/history`, `/tool-contracts`, `/shell-ask`, `/model`).
- `architecture/command.md` — counts 142→149, typed execution flow,
  `Command`/`CommandAction` API sections, representative table fixed
  (aliases, new commands, `/checkpoint` removal note, stale rows
  dropped), invariant documents the docs-count guard.

No storage, protocol, daemon, scheduler, authorization, or
permission/sandbox semantics were added or changed. No public command
was renamed; `/model`, `/checkpoints`, `/history` persist as documented
aliases.

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg --lib tui::command::tests
cargo test -p codegg --lib tui::
cargo test --test tui
cargo test -p codegg --lib -- tui::app::state::observe
cargo test -p codegg --lib -- command::
cargo test -p codegg --lib -- palette
cargo test -p codegg --lib -- task_view
cargo test -p codegg --lib -- workspace_dashboard
cargo test -p codegg --lib -- checkpoint
cargo test -p codegg --lib -- undo
cargo test -p codegg --lib -- plugin_management
cargo test -p codegg --lib -- worktree
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
git diff --check
```

### Results

- `tui::command::tests`: 18 passed (10 new/strengthened).
- `tui::` lib suite: 936 passed, 0 failed.
- `tui` integration: 165 passed, 0 failed.
- Observer state: 7 passed, including updated control-family blocking.
- Palette (3), task view (3), workspace dashboard (30), checkpoint (5),
  undo (2), plugin management (29), worktree (6): all passed.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed.
- `scripts/verify.sh quick`: passed (fmt, builtin-agents check,
  core-boundary, sandbox, execution-ownership, tui-authority, workspace
  check).
- `git diff --check`: passed.
- Full workspace `cargo test` and `verify.sh full` were not run; the
  plan's verification scope is the focused suites above plus quick.

## 5. Invariant review

- User scripts do not break without a path: no public command renamed;
  former dispatcher-only spellings (`/model`, `/checkpoints`,
  `/history`, `/fullscreen`, `/voice`, etc.) now resolve through
  registry aliases, strictly widening reachability.
- Authorization/observer/permission checks remain downstream: the
  central observer guard runs before typed dispatch; the allowlist is
  fail-closed and the reconciled commands test as blocked.
- Dynamic project/plugin precedence unchanged: built-ins win
  deterministically; aliases participate in reservation; tests cover
  collisions.
- Discovery performs no new I/O on the hot path: resolution is the
  existing in-memory `find_by_name_or_alias`; no filesystem reads added.
- CLI surface untouched: M002 owns it; this milestone changes no Clap
  surface.
- Default-build help unaffected beyond the reconciled commands now
  appearing in discovery (previously hidden but executable, or
  discoverable but inert).

## 6. Failure and recovery review

No asynchronous or durable operation was added. Dispatch is synchronous
routing to pre-existing handlers; cancellation, restart, lease,
idempotency, and bounded-output behavior of those handlers is unchanged.
Ambiguous catalogs still fail `validate()`; invalid slash input still
falls through `handle_slash_command` as non-command text. Plugin
commands remain discovery entries with an explicit bounded toast rather
than silent no-op. The removed `/checkpoint` name now resolves as
non-command text (palette shows no match; typed input sends as prompt
text), which is the evidence-backed disposition, not a silent redirect.

## 7. Migration and compatibility review

No schema, protocol, or config migration. Legacy names persist as
aliases (`/model`, `/checkpoints`, `/history`, plus pre-existing
`quit`/`q`/`clear`/`fullscreen`/`voice`/etc., which now resolve
through the same typed actions). The `/checkpoint` removal is the only
non-alias disposition; it had no executable behavior to preserve, so
there is nothing to migrate. `Command::new` is an in-crate API whose
signature change (now takes `CommandAction`) affects only
`src/tui/command.rs`.

## 8. Security review

Discovery metadata remains non-authoritative. The observer fail-closed
allowlist was extended in tests to the reconciled set; no reconciled
command is allowlisted. Secrets, paths, and workspace containment flow
through the pre-existing per-handler code (unchanged). The deleted
`/search` special-case in `handle_slash_command` previously bypassed
the central observer guard; `/search` now routes through it (it is
allowlisted, so behavior is preserved with the guard in place).

## 9. Documentation and operations

- `architecture/command.md` updated (counts, typed flow, API, table,
  invariant) with a parsing docs-count guard test.
- Implementation plan status `ready` → `active` (implementation commit)
  → `implemented` (this closure).
- Subsystem roadmap M001 `ready` → closed; roadmap stays active for
  independent M002.
- `plans/registry.md` updated (subsystem row, M001 closed, M002 ready).
- No new CI lane, scanner, gate, bot, or release automation added.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Built-in arm bodies still parse args from `command_palette.query` in many handlers rather than from a single parsed-args value threaded through `dispatch_builtin_command`. | Palette and direct-typed paths agree today only because `handle_slash_command` syncs the query; a future handler reading stale query state could regress. | Follow-up polish (not corrective): thread one parsed-args value into built-in handlers. |
| low | `CommandAction::Plugin` is discovery + bounded toast; plugin slash commands do not execute handlers. | Preserves M010 scope; no regression. | Plugin execution surface, if ever desired, is separate work. |

No critical, high, or medium findings remain.

## 11. Roadmap disposition

Milestone closed. The corrective roadmap stays active for independent
M002 (CLI truthfulness and compatibility cleanup), which this closure
does not block or complete.

## 12. Registry updates

- Move M001 from active to recently closed with implementation
  `6e5203b6` and this closure record.
- Keep M002 `ready` (independent of M001; no dependency change).
- Keep the corrective roadmap `active` until M002 closes.
- Blocked-work audit: no registered plan lists M001 as a hard or
  interface dependency. M002 is independent per the roadmap. The
  standing blockers (dependency-security M005 external updater
  interface; architecture-convergence M009 operational evidence;
  runtime-safety C002 Linux fixture) are unrelated and remain blocked.
  No future plan is unblocked by this closure; none is newly blocked.
