# TUI Modularity and Render Invariants Milestone 010 — Slash dispatch tables

Status: conditionally closed

Repository baseline: `7642b9e38` on `feat-auto-20261109-f8abad0d`

Source roadmap:

- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md#milestone-010--slash-dispatch-tables`

Long-term requirements:

- `plans/000-long-term-specification.md#29-system-invariants` — invariant 24:
  the daemon owns one command catalog, and every command that changes
  daemon-owned state is authorized in the daemon and produces a `Run`. This
  milestone reshapes frontend **dispatch** and does not touch catalog
  ownership.

Applicable ADRs:

- `plans/adrs/ADR-0052-cross-frontend-command-execution-ownership.md` —
  accepted. The catalog is daemon-owned. This milestone changes where the
  frontend's arm bodies live, not what a command is or who authorizes it.

Primary class: infrastructure

## 1. Objective

Split the 161-arm, 3,101-line `dispatch_builtin_command` into family modules
behind a delegation table, so adding a command stops meaning editing a
three-thousand-line function — while keeping the match exhaustive so a missing
arm is a compile error rather than a silent no-op.

This milestone delivers **no user-visible capability**. Its acceptance evidence
is that the corpus did not move and that every action is still reachable.

## 2. Why this milestone is ready

Blocked on M001 (hard, verification gate). Independent of every other milestone:
command routing touches neither chrome, nor hints, nor frame geometry, nor list
state.

The catalog half of the contract already exists: `src/tui/command.rs` defines
`BuiltinSlashAction` and `built_in_commands()`, and
`scripts/check_command_catalog_ownership.py` enforces that the daemon's catalog
and the TUI's surface are the same set. This milestone does not add a command,
so the catalog and the guard are untouched.

## 3. Current implementation evidence

At baseline `7642b9e38`:

- `App::dispatch_builtin_command` (`src/tui/app/mod.rs:5800`) is **3,101
  lines** containing **161 match arms** over `BuiltinSlashAction`.
- The arms are already half-decomposed: **23 distinct `commands::*` modules** are
  called from it (`chat` 14, `interactive_terminal` 14, `policy` 11, `agents` 7,
  `workspace_dashboard` 7, `plugin_management` 7, `reasoning` 5, and twelve more
  with 1–3 each). The remaining arms call `self.` inline — **101 such calls** —
  and those are the logic that has nowhere else to live.
- A three-line teardown is repeated across the function:
  `command_mode = false` appears **95 times** in `src/tui`, and **30** of those
  are the identical `command_mode / prompt.clear / show_completions` triple, all
  in `app/mod.rs`.
- `src/tui/command.rs` is 2,234 lines, of which `built_in_commands()` is 421 —
  the catalog, which is the other half of the command surface and is not in
  scope.
- `App` has 330 methods in one `impl`, of which this dispatcher is the largest
  single body.
- `scripts/check_command_catalog_ownership.py` and
  `scripts/check_command_argument_contract.py` both constrain this surface and
  must pass unmodified.

## 4. Invariants that must not regress

- **The match stays exhaustive.** A new `BuiltinSlashAction` variant must fail to
  compile until it has an arm. This is why the milestone uses a delegation
  *match*, not a runtime `HashMap` — a map would trade a compile error for a
  silent no-op, which is strictly worse.
- **The catalog is unchanged.** No command added, removed or renamed.
  `check_command_catalog_ownership.py` and `check_command_argument_contract.py`
  pass unmodified.
- **Argument handling is unchanged**, including the
  `argument.unwrap_or_default().trim()` normalisation at the top of the current
  function and any per-command parsing.
- **Every action remains reachable** and is covered by a test.
- **The corpus is byte-identical.** Command behaviour produces the same output.
- **I1–I6 stay green.**
- **The comment ratchet holds.** This milestone *reduces* comment mass in
  `app/mod.rs`, which must be banked, and adds mass in the family modules, which
  must be listed.

## 5. Scope

### In scope

- Family modules under `src/tui/commands/`, one per command family, each
  exposing a small entry point that takes the `App`, the action and the
  normalised argument.
- Converting the inline `self.` arms into family functions.
- A shared helper for the repeated command-mode teardown.
- The delegation match in `dispatch_builtin_command` reduced to one arm per
  variant.
- A test that every action is reachable.

### Explicitly out of scope

- `dispatch_tui_command` (`src/tui/runtime/command_dispatch.rs:85`) — 3,210
  lines over 259 async `TuiCommand` variants. Deferred by roadmap §3: its arms
  own task spawning, so it needs an async-handler contract as well as a table.
- The catalog in `src/tui/command.rs`.
- Any change to argument parsing semantics.
- Any change to command authorization, which is daemon-side (ADR-0052).

## 6. Required production changes

### Core/domain

None.

### Storage and migrations

None.

### Protocol and DTOs

None. No `TuiCommand` variant is added or changed.

### Runtime and concurrency

None. The converted dispatcher stays synchronous. Family functions must not
spawn a task that the current arm does not spawn.

### Frontend or operator surface

- `src/tui/commands/` gains family modules. Each family's entry point is
  narrow: it receives what the arm received, and returns what the arm returned.
- `dispatch_builtin_command` keeps its signature and its argument
  normalisation, and becomes a delegation match.
- The teardown helper replaces the 30 identical triples.

### Security and authorization

None. The milestone must not move a check from the arm into a family function
in a way that changes its order relative to the command's effect; a family
function performs the same sequence.

### Documentation and static guards

- `architecture/command.md` or `architecture/tui.md` — the delegation rule: a
  new command adds one arm and one family function, and the family owns the
  body.
- `docs/comment-budget.toml` — bank the reduction in `app/mod.rs`, list the new
  files.

## 7. Ordered work packages

### Work package A — Characterise reachability

Intent: pin that every action does what it does today before moving any body.

Required changes: a test that constructs every `BuiltinSlashAction` variant and
asserts it reaches a handler — either a state change or a delegated command —
rather than falling through. Where an action legitimately does nothing in the
current tree, record it explicitly rather than asserting a no-op.

Acceptance evidence: the test passes on the unmodified baseline. That is the
proof it characterises rather than specifies.

### Work package B — The teardown helper

Intent: remove the 30 identical triples first, because it is mechanical and it
shrinks the surface the later packages move.

Required changes: one helper that closes command mode, clears the prompt and
hides completions; every triple uses it.

Acceptance evidence: the corpus is byte-identical and the 30 sites are gone.

### Work package C — The largest families

Intent: move the arms that already delegate, so the pattern is proved on the
easy majority.

Required changes: `chat` (14), `interactive_terminal` (14), `policy` (11),
`agents` (7), `workspace_dashboard` (7) and `plugin_management` (7) get their
inline arms moved into their existing modules.

Acceptance evidence: corpus byte-identical; `dispatch_builtin_command` shrinks
by roughly those arm counts.

### Work package D — The remaining families

Intent: finish the split.

Required changes: the remaining 1–5-arm families, including creating modules for
families that have none.

Acceptance evidence: every arm is a one-line delegation; the function is under
250 lines.

### Work package E — Exhaustiveness proof

Intent: make the compile-time guarantee explicit.

Required changes: a test or documented check proving that adding a variant
without an arm fails to compile. Because that cannot be a test, the closure
record carries the evidence: a temporary variant added locally, the compiler
error quoted, and the variant removed.

Acceptance evidence: the quoted compiler error.

## 8. Failure, cancellation, restart, and contention semantics

Not applicable to the conversion itself. One semantic property must survive: an
arm that enqueues a `TuiCommand` must still enqueue it at the same point
relative to any state it mutates first. Reordering "set state" and "enqueue" can
change what the enqueued command observes. The family function performs the same
sequence in the same order, and the reachability test asserts the resulting
state, not just that a command was enqueued.

## 9. Compatibility and migration

No compatibility surface. Every command behaves identically. The migration is
internal and complete in one milestone: no arm is left behind a flag or a
fallback path.

## 10. Required tests

### Focused unit tests

- The teardown helper clears all three fields.
- Each family entry point handles its action and argument, including the empty
  and whitespace-only argument cases.

### Integration tests

- The reachability test over every `BuiltinSlashAction` variant.
- A representative end-to-end command per family produces the same state as
  before.

### Restart and recovery tests

None.

### Contention and cancellation tests

None. No async boundary moves.

### Security and negative tests

- An action whose arm performs a check before its effect still performs the
  check before the effect, asserted by a test that the effect does not happen
  when the check fails.
- An unknown or empty argument behaves as it does today.

### Migration and compatibility tests

- `check_command_catalog_ownership.py` and
  `check_command_argument_contract.py` pass unmodified.

## 11. Required verification commands

```bash
# the dispatcher and its families
cargo test --lib -- tui::app::commands
cargo test --lib -- tui::commands

# the catalog and argument contracts, unmodified
python3 scripts/check_command_catalog_ownership.py
python3 scripts/check_command_argument_contract.py

# the frontend action set
python3 scripts/check_frontend_action_set.py

# the render-cost shape guard
python3 scripts/check_tui_render_cost.py

# the comment ratchet, which this milestone changes in both directions
python3 scripts/check_comment_budget.py

# formatting and linting
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings

# the routine gate
scripts/verify.sh quick
```

## 12. Documentation updates

- `architecture/tui.md` — the delegation rule for a new command.
- `docs/comment-budget.toml` — bank the `app/mod.rs` reduction and list the new
  family modules.

## 13. Acceptance criteria

1. `dispatch_builtin_command` is a delegation match under 250 lines.
2. The match remains exhaustive, proved by a quoted compiler error for a
   variant without an arm.
3. Every `BuiltinSlashAction` variant is reachable and tested.
4. The 30 teardown triples are one helper.
5. The corpus is byte-identical.
6. The catalog and argument-contract guards pass unmodified.
7. No command is added, removed or renamed.
8. `scripts/verify.sh quick` is green.

## 14. Stop conditions

Report rather than improvise when:

- an arm's body cannot move without taking `&mut self` state the family module
  cannot see — report the coupling rather than widening the family signature
  until it is a second `App`;
- an arm's order of operations is ambiguous enough that a test cannot pin it;
- a family module would need to be public beyond the crate to be reachable;
- the comment bank would be exceeded because the split adds more comment mass
  than it removes — report the numbers rather than trimming prose to fit;
- a command turns out to be reachable only through this dispatcher and has no
  test — record it before moving it, so the move cannot silently break it.

## 15. Closure evidence required

- The reachability test, passing on baseline and after.
- The delegation match's final line count.
- The quoted compiler error proving exhaustiveness.
- Corpus byte-identical result.
- Catalog and argument-contract guard output, unmodified.
- `quick` output.
- A closure record at
  `plans/closure/tui-modularity-and-render-invariants/010-status.md`.

## 16. Handoff notes

Keep the match. The instinct will be to replace 161 arms with a table of
`(action, handler)` pairs, which reads beautifully and throws away the only
thing the compiler currently guarantees: that a new variant is handled. A
delegation match gives the same one-line-per-arm readability and keeps the
error.

Move the delegating arms first. The six largest families already call into
`commands::*`, so their conversion is mostly deleting a body that was already
in the right module. Proving the pattern there makes the remaining families
mechanical.

Bank the comment budget honestly. This milestone removes a large amount of
comment mass from `app/mod.rs` and adds a smaller amount across new files; the
global ceiling must not rise, and the per-file ceilings must be recorded in the
same change. A split that leaves the manifest stale fails `quick` for a reason
that has nothing to do with the refactor.
