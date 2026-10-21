# TUI Subagent Presentation and Name Allocation — Milestone P003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-subagent-presentation-and-name-allocation/003-sessions-panel-navigation-skips-todo-rows.md`

Source subsystem roadmap:

- `plans/subsystems/tui-subagent-presentation-and-name-allocation-corrective-addendum.md` §7 (P003)

Repository baseline reviewed: `31a412844fbbf36055b29245075121035eefc385`

Implementation commits or pull requests:

- Working tree only — not committed at the time of writing. The change is
  confined to `src/tui/components/sidebar.rs`, `tests/tui_side_panels.rs`,
  `docs/tui.md`, `architecture/tui.md`, and `.opencode/skills/tui/SKILL.md`,
  plus the plan records named above.

## 1. Executive finding

The `SESSIONS` side panel's keyboard walk stopped on rows that **did nothing**.
`focus_targets()` pushed a `SidebarFocusTarget::Todo(i)` for every rendered
todo, `activate_focused()` resolved that target to `SidebarActivation::None`,
and `with_row_style` painted the selection highlight on the focused row. So
`j`/`k`/arrows walked through every todo in turn, the highlight landed on a row
where `Enter` was a no-op, and the key press was swallowed by the panel — the
user's "the focus is on something and nothing happens" symptom.

The focus list is now built from the rows whose activation is not a no-op, and
the `SidebarFocusTarget` variants are exactly that set: the `Todo` variant was
**removed** rather than left unreachable, because a variant the list cannot
produce is a variant that lies. The `Todos` **section header** stays focusable
and still folds the section — the request was to skip the rows, not the header.

Both traps the plan named were closed *as traps*, not merely as code:

- **The hover→focus trap.** A hovered todo now resolves to the **section
  header**, a target the focus list can produce. It deliberately does *not*
  resolve to nothing: `focus_hovered` would then clear the focus outright, and
  the next navigation key would fall through `move_focus`'s no-current branch
  and jump to index 0 — the panel yanking to the top on every click of a todo
  row. The proof that this is not a hypothetical is in §4.2: the new hover test
  **fails** against a partial fix that drops the rows from the list but leaves
  the old hover mapping in place.
- **The dead arm.** `ensure_focused_visible`'s `Todo` arm went with the
  variant, and the compiler flagged the leftover as unreachable.

The mouse map (`line_targets`) is untouched: a todo row is still hovered, still
shows its tooltip, and still resolves to the todo on a click. The `Ctrl+T`
overlay's selection model was **not** touched — there a todo row *is*
actionable, so it stays selectable there, and the divergence is recorded as a
decision rather than "fixed" for consistency.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| AC1 — `j`/`k`/arrows never land on a todo row | `focus_targets()` (`sidebar.rs:667`) contributes the Todos **header** and nothing else; `a_todo_row_is_never_the_focused_target` (`sidebar.rs:2418`); integration test `the_session_panel_walk_skips_todo_rows_but_keeps_their_header` (`tests/tui_side_panels.rs:989`) | pass | the focus list holds one Todos entry for two rendered todos |
| AC2 — the Todos header remains focusable and Enter collapses/expands the section | `activate_focused`'s `Section(_)` → `ToggleSection` arm is unchanged (`sidebar.rs:618`); `the_todos_header_is_focusable_and_folds_the_section` (`sidebar.rs:2462`) | pass | |
| AC3 — the selection highlight is never painted on a todo row | the highlight follows `focused_target` (`with_row_style`); a todo row can no longer be a focus target, so it can no longer carry the highlight | pass | follows from AC1 by construction |
| AC4 — clicking a todo row does not clear the focus and does not make the next key jump to the top | `hovered_to_focus` resolves a hovered todo to the header (`sidebar.rs:802`); `clicking_a_hovered_todo_leaves_a_valid_focus` (`sidebar.rs:2492`) | pass | the test **fails** against the trap (§4.2) |
| AC5 — mouse hover and tooltips on todo rows still work; `line_targets()` unchanged | `line_targets()` still pushes `HoveredElement::Todo(i)` (`sidebar.rs:1091`); `the_mouse_map_still_resolves_todo_rows` (`sidebar.rs:2535`) | pass | no line in `line_targets` was touched |
| AC6 — the `Ctrl+T` overlay's selection model is untouched | no file under `src/tui/app/state/tasks.rs` was modified; `scripts/check_tui_transport_authority.py` is green | pass | the divergence is recorded in `docs/tui.md` and `architecture/tui.md` |
| AC7 — `scripts/verify.sh quick` is green | see §4.3 | pass | |

## 3. Production implementation evidence

| File | Change | Anchor |
|---|---|---|
| `src/tui/components/sidebar.rs` | the `SidebarFocusTarget::Todo(usize)` variant is **removed**, and the enum's doc comment now states the invariant: every variant is a row that can be focused **and** has an action | `:32`, `:43` |
| | `focus_targets()` drops the todo rows and keeps the Todos header | `:667` |
| | `activate_focused()` loses its `Todo` arm | `:637` |
| | `ensure_focused_visible()` loses its `Todo` arm | `:500` |
| | `hovered_to_focus()` resolves a hovered todo to the **Todos header**, never to a target the list cannot produce and never to nothing | `:802` |
| | `line_targets()` — the **mouse** map — is unchanged and keeps its todo entries | `:1091` |
| `tests/tui_side_panels.rs` | new integration test: the walk's sequence of focused identities over a fixture with todos, subagents and jobs | `:956` |
| `docs/tui.md` | §"Session panel sections" — the walk skips rows with no action; the Todos heading still folds the section; the mouse path and the `Ctrl+T` divergence | `:798` |
| `architecture/tui.md` | the side-panel focus model — invariant I5, what the focus list is, and why the hover mapping resolves to the header | `:3747` |
| `.opencode/skills/tui/SKILL.md` | new bullet: a focusable row has an action; the mouse map is a different map; the trap | `:1055` |

### 3.1 Deviations from the plan, and why

1. **The pinned focus-walk test was renamed**, from
   `focus_walks_todo_then_subagent_then_job_rows_without_collapsing` to
   `focus_walks_the_todos_header_then_subagent_then_job_rows_without_collapsing`.
   The plan required the test to be "updated to the new order, not deleted and
   not weakened"; the name asserted a walk that no longer happens, and a test
   whose name is false is a liability. Every other assertion it made is kept:
   a subagent row is reached, then a job row, and no section collapses on the
   way (the focus list's length is still asserted).
2. **The hover mapping resolves to the header, not to `None`.** The plan
   allowed both ("mapping it to `None` (leave the focus where it is) is
   acceptable"). `None` is **not** acceptable in this widget: `focus_hovered`
   assigns the result to `focused_target` directly, so `None` clears the focus
   rather than leaving it where it was, and the next navigation key then lands
   on index 0. The header is the only choice that is both present in the focus
   list and keeps the focus alive. §4.2 proves the trap is real.
3. **The "no todo row is ever focused" test is written as a property of the
   list, not as a comparison against a removed variant.** The plan's phrasing
   ("a todo row is never the focused target") cannot be written literally once
   the `Todo` variant is gone — there is no value to compare against. The test
   asserts the equivalent and stronger thing: the list holds **one** Todos
   entry for two rendered todo rows, and every stop of a full forward and
   backward walk is on the list.
4. **The `SidebarFocusTarget::Todo` variant's removal is a `pub` API change.**
   The plan required it (work package A, step 2). The variant was public, and
   removing it is a breaking change to a public enum — a deliberate one, since
   the milestone's whole point is that the type should say what it means. The
   type is not used outside `src/tui/components/sidebar.rs` and the TUI's own
   tests, so nothing else had to change.
5. **`ensure_focused_visible`'s arm is gone for a compiler reason, not a
   stylistic one.** With the variant removed the match arm was unreachable, and
   the compiler flags it — which is the plan's stated acceptance evidence for
   work package C.

## 4. Verification executed

### 4.1 Commands run

```bash
# narrow tests first
cargo test -p codegg --lib tui::components::sidebar
cargo test -p codegg --test tui_side_panels

# static guards
python3 scripts/check_tui_transport_authority.py

# formatting and linting
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings

# the broader sweep
scripts/verify.sh quick
```

### 4.2 Baseline-failing evidence (non-vacuity)

The proof was run in **two** modes, both keeping the new tests and reverting
only `src/tui/components/sidebar.rs`.

**Mode `all` — the full pre-P003 focus model.** Todo rows are focus targets
again, the variant is back, and the hover mapping is the old one.

| Test | Result |
|---|---|
| `focus_walks_the_todos_header_then_subagent_then_job_rows_without_collapsing` | **FAILED** — `left: [Section(Todos), Todo(0), Section(Subagents), Subagent("t1"), Section(Jobs)]`, `right: [Section(Todos), Section(Subagents), Subagent("t1"), Section(Jobs), Job("j1")]` |
| `a_todo_row_is_never_the_focused_target` | **FAILED** — `left: 7`, `right: 5` (the two rendered todo rows were stops) |
| `the_session_panel_walk_skips_todo_rows_but_keeps_their_header` (integration) | **FAILED** |

Totals: `cargo test -p codegg --lib tui::components::sidebar` **20 passed, 2
failed**; `cargo test -p codegg --test tui_side_panels` **31 passed, 1 failed**.

**Mode `trap` — only the hover path reverted.** The todo rows stay out of the
focus list, and the old hover mapping is put back: the trap the plan warns
about, with the list already changed.

| Test | Result |
|---|---|
| `clicking_a_hovered_todo_leaves_a_valid_focus` | **FAILED** — `clicking a todo row must not clear the focus` |
| every other sidebar test | 21 passed, 0 failed |
| the integration test | 32 passed, 0 failed |

That second mode is the point: the hover test passes against **both** the
pre-P003 state and the final state, and fails only against the trap. It is a
guard against a defect the fix itself could introduce, and the proof shows the
defect is real rather than imagined.

**The old and new expected orders, quoted.** The walk was

```
[Section(Todos), Todo(0), Section(Subagents), Subagent("t1"), Section(Jobs), Job("j1")]
```

and is now

```
[Section(Todos), Section(Subagents), Subagent("t1"), Section(Jobs), Job("j1")]
```

The todo **row** is gone from the walk; the Todos **header** is not.

### 4.3 Results

| Command | Result |
|---|---|
| `cargo test -p codegg --lib tui::components::sidebar` | 22 passed, 0 failed |
| `cargo test -p codegg --test tui_side_panels` | 32 passed, 0 failed |
| `cargo test -p codegg --lib tui::` | 2585 passed, 0 failed |
| `cargo test -p codegg --test tui_main_buffer` | 70 passed, 0 failed |
| `cargo test -p codegg --test tui_render` | 111 passed, 0 failed |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean, no warnings |
| `python3 scripts/check_tui_transport_authority.py` | pass |
| `scripts/verify.sh quick` | pass — "Quick verification passed." |

### 4.4 Not run

- `scripts/verify.sh full` — the nextest workspace sweep and the
  `--features server,plugins,lsp-test-support` pass were **not** run. The
  change is confined to a TUI widget's focus model and touches no protocol,
  storage, server, or scheduler path.
- `cargo test -p codegg` in full (the whole crate) was **not** run; the TUI
  integration binaries that cover this surface were.

**One pre-existing test is load-sensitive and was observed to fail under
concurrent load, unrelated to this change.**
`src/tui/commands/interactive_terminal.rs:3461`
(`a_hidden_terminal_surface_does_not_arm_the_loop`) waits for the terminal
create command in a single 25 ms drain; the file's own doc comment at
`src/tui/commands/interactive_terminal.rs:3033` records that one drain "is not
enough". It failed twice while a 2 580-test sweep was running concurrently in
the same worktree, and passed 8/8 in isolation on an idle machine **both** at
the baseline and with the P002 and P003 changes applied. The final TUI-filtered
lib sweep (§4.3) was run on an idle machine and is green. It is recorded here
rather than left to be discovered as a surprise.

### 4.5 Wider sweep

`scripts/verify.sh quick` was run on the final tree state and passed
("Quick verification passed."). It covers the agent schema, the
core/client/desktop boundaries, the sandbox, execution-ownership, TUI authority,
http-route-disposition, audit-coverage, queue-delivery-callsites,
queue-write-transaction-shape, scheduler-bypass, config-merge-coverage, the
no-hardcoded-models guard, the provider wire/catalog/resilience guards, the
OpenAI endpoint guard, Eggwork target routing, and a workspace `cargo check`.

## 5. Invariant review

- **I5 — a focusable row has an action.** The focus list is built from the
  rows whose activation is not a no-op, and the `SidebarFocusTarget` variants
  are exactly that set. The type now says what it means.
- **The Todos header still folds the section.** `Section(_)` →
  `SidebarActivation::ToggleSection` is untouched.
- **Collapsing a section still hides its rows from the focus list** — the
  guard that used to sit on the todo rows now sits on every other section's
  rows, unchanged.
- **`reconcile_focus` still copes with the list changing underneath it.** The
  focused target is now a header or a row of another kind, and the fallback
  that keeps the focus valid is unchanged.
- **Mouse hover, tooltips, and click resolution on todo rows still work.**
  `line_targets()` was not modified, and the test asserts the map still
  resolves a todo row.
- **The `Ctrl+T` overlay is deliberately different.** There, a todo row *is*
  actionable and stays selectable. The two surfaces differ on purpose, and
  the divergence is recorded in `docs/tui.md` and `architecture/tui.md`.

## 6. Failure and recovery review

No state, async, or cancellation behaviour changes. The one behavioural change
is which rows the keyboard walk stops on. Nothing that was reachable before is
unreachable now: the Todos header is still a stop, and every other section's
rows are unchanged. The only rows that left the walk are rows whose `Enter` did
nothing.

## 7. Migration and compatibility review

No schema, protocol, or configuration change. The compatibility surface is
one `pub` enum variant, `SidebarFocusTarget::Todo(usize)`, removed. It is used
nowhere outside `src/tui/components/sidebar.rs` and the TUI's own tests, and
the removal is the milestone's stated intent: the type is the focus model, and
a focus target that cannot be focused does not belong in it.

## 8. Security review

No new input, no new parsing, no new external call, no new authority. The
milestone removes a code path rather than adding one. A hovered todo resolving
to the section header is a *narrowing* of what a click can select, not a
widening.

## 9. Documentation and operations

- `docs/tui.md` §"Session panel sections" — the walk skips rows with no
  action; the Todos heading still folds the section; the mouse path is a
  different map; the `Ctrl+T` overlay is the deliberate opposite.
- `architecture/tui.md` — the side-panel focus model, invariant I5, and the
  reasoning behind the hover mapping.
- `.opencode/skills/tui/SKILL.md` — one new bullet, so the skill and the
  architecture doc move together as `AGENTS.md` requires.
- No user-facing operational change: no new key, no new command, no new
  configuration. The only visible difference is a shorter `j`/`k` walk.

## 10. Unresolved findings

- None. The two traps the plan named are closed and proved closed, and the
  deliberate divergence from the `Ctrl+T` overlay is recorded rather than
  left for the next reader to rediscover.
- The load-sensitive terminal test described in §4.4 is **pre-existing** and
  was not fixed here; it is not in this milestone's blast radius.

## 11. Roadmap disposition

**P003 is closed.** P001 and P002 stay closed. **P004** — the two-pass name
allocation — is unaffected by this milestone and remains the last open item in
the addendum; it touches `src/agent/subagent_names/registry.rs` and the
generated name list, which are disjoint from everything P001–P003 changed.

The addendum's status table and `plans/registry.md` are updated in §12.

## 12. Registry updates

- `plans/registry.md` — the P003 row moves to **closed**, with this closure
  path and the evidence for the seven acceptance criteria; the addendum's own
  row records that P001–P003 are closed and that P004 is the remaining item.
- `plans/subsystems/tui-subagent-presentation-and-name-allocation-corrective-addendum.md`
  — the §12 status table gives P003 its closure path; §4.7 is marked as a
  **baseline record** so its citations stay pinned to `31a4128`; the
  deliberate divergence from the `Ctrl+T` overlay is kept and cross-referenced.
- `plans/implementation/tui-subagent-presentation-and-name-allocation/003-sessions-panel-navigation-skips-todo-rows.md`
  — status → `closed`, with an `Outcome:` paragraph, and §3 marked as a
  baseline record.
