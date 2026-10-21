# TUI Subagent Presentation and Name Allocation — Milestone P003 — The Side Panel Navigates Actionable Rows Only

Status: closed — closure `plans/closure/tui-subagent-presentation-and-name-allocation/003-status.md`

Outcome: the side panel's keyboard walk now stops only on rows that **do
something**. Todo rows left the focus list, and the `SidebarFocusTarget::Todo`
variant was removed with them — a variant the list cannot produce is a variant
that lies, and the model's meaning is now "a focusable row has an action"
(invariant I5). The `Todos` **header** stays focusable and still folds the
section: the request was to skip the rows, not the header.

The two traps the plan named were closed as traps, not just as code. A hovered
todo now resolves to the **section header** — a target the focus list can
produce — and never to nothing: resolving it to nothing clears the focus, and
the next navigation key then falls through `move_focus`'s no-current branch and
jumps to index 0, yanking the panel to the top. The now-dead
`ensure_focused_visible` arm is gone with the variant, and the mouse map
(`line_targets`) keeps its todo entries, so hover, tooltips, and click
resolution on a todo row are unchanged. The `Ctrl+T` overlay's selection model
was **not** touched: there a todo row is actionable, so it stays selectable
there, and the divergence is recorded as a decision.

The closure record's §4.2 holds the evidence, including the proof that the
hover test is not vacuous: it fails against a *partial* fix that drops the
todo rows from the list but leaves the old hover mapping in place.

Repository baseline: `31a412844fbbf36055b29245075121035eefc385`

Source roadmap:

- `plans/subsystems/tui-subagent-presentation-and-name-allocation-corrective-addendum.md` §7 (P003)

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles` — §4.6 *Progressive disclosure*, §4.7 *Correctness before transparent magic*.

Applicable ADRs: none.

Primary class: **capability** (K2), delivering invariant **I5** — *a focusable row has an action*.

## 1. Objective

In the `SESSIONS` side panel, `j`/`k`/arrows move between rows that **do something**, and the "Todos" section header stays selectable so the section can still be folded.

Today keyboard focus walks straight through todo rows, the selection highlight lands on them, and Enter on one does nothing at all.

## 2. Why this milestone is ready

- **Hard dependency: P002** (`002-tab-bar-identity-persona-and-hints.md`). P002 moves the persona onto the tab strip and removes the descriptor from this same panel's subagent row. Landing P003 first would leave the persona on no summary surface at all. P002 must be **closed**, not merely started.
- **No other hard dependency.** No protocol, projection, storage, or execution path is involved.
- **Interface dependency:** none.

## 3. Current implementation evidence

> This section is the **baseline record** at `31a4128`, kept as the plan wrote
> it. Its line citations are pinned to that commit and are deliberately **not**
> refreshed: they describe the state the plan was written against, and the
> change this milestone made is the difference between them and the tree. The
> closure record's §3 carries the post-change anchors.

### 3.0 Which "sessions" surface this milestone means

The **side panel** — `SidePanelId::Session`, titled `SESSIONS`, implemented in `src/tui/components/sidebar.rs`. That is where the todo rows live, where the selection highlight is painted, and where `j`/`k`/arrows navigate.

**Not** the `/sessions` **switcher dialog** (`Dialog::Session`), whose own cursor model is a different widget and is out of scope. Both surfaces are called "sessions" and the parent workstream was misled by exactly this once; the `/sessions` dialog's selection model is left untouched here.

### 3.1 Todo rows are focus targets

`focus_targets()` (`src/tui/components/sidebar.rs:658-743`) builds the keyboard focus list. For todos:

```rust
if !self.todos.is_empty() {
    targets.push(SidebarFocusTarget::Section(SidebarSection::Todos));
    if !self.todos_collapsed {
        targets.extend((0..self.todos.len()).map(SidebarFocusTarget::Todo));
    }
}
```

(`sidebar.rs:670-675`.) The section header and the rows are pushed together.

### 3.2 Enter on a todo row does nothing

`activate_focused()` (`sidebar.rs:619-647`):

```rust
Some(SidebarFocusTarget::Todo(_))
| Some(SidebarFocusTarget::McpServer(_))
| Some(SidebarFocusTarget::FileChange(_))
| Some(SidebarFocusTarget::ToolProgram(_))
| Some(SidebarFocusTarget::Convergence(_)) => SidebarActivation::None,
```

(`sidebar.rs:640-644`.) A todo row's activation is `None` — the activation is *consumed* by the panel and produces no action (`src/tui/panels/session.rs:81-99` maps `Send` → `activate_focused()`).

`with_row_style` (`sidebar.rs:2015-2021`) paints the selection highlight on the focused row, so the highlight is painted **on a row whose Enter is a no-op**.

### 3.3 How navigation reaches them

`src/tui/panels/session.rs:66-67` maps `NavigateUp`/`NavigateDown` to `focus_prev`/`focus_next` (`sidebar.rs:554-560`), which call `move_focus` (`sidebar.rs:745-762`) over `focus_targets()`. So `j`/`k`/arrows walk through every todo row in turn.

### 3.4 Two traps the fix creates (they are **not** in today's code)

- **The hover→focus trap.** `hovered_to_focus` (`sidebar.rs:792-799`) maps `HoveredElement::Todo(index)` to `SidebarFocusTarget::Todo(index)` (`:795`). `focus_hovered()` (`sidebar.rs:610-617`) is called on **click** (`src/tui/app/mod.rs:9266`). If todo rows leave `focus_targets()` but that mapping survives, clicking a todo row sets `focused_target = Some(Todo(i))`, which is no longer in `focus_targets()`, so `focused_position` becomes `None` and `reconcile_focus()` (`sidebar.rs:764-781`) **clears the focus entirely**. The next `j` press then hits `move_focus`'s `None` branch and jumps to **index 0** — the focus yanks to the top of the panel.

  The fix: a hovered todo must map to a target that is **present in `focus_targets()`**, or to `None` — never to an absent one. Mapping a hovered todo to the **Todos section header** is the natural choice (it is the row that owns the todo rows); mapping it to `None` (leave the focus where it is) is acceptable. Both are fine; an absent `Todo` target is not.

- **The dead arm.** `ensure_focused_visible`'s match on `SidebarFocusTarget` includes a `Todo(index) => HoveredElement::Todo(index)` arm (`sidebar.rs:504`). Once todo rows leave the focus list, that arm can never be reached.

### 3.5 `line_targets()` is a different map and must not change

`line_targets()` (`sidebar.rs:1011+`, todo at `:1084`) is the **mouse hit-test / tooltip** map — it is what `element_at` (`sidebar.rs:979-981`) indexes to resolve a mouse position to a `HoveredElement`. Todo entries **stay**: hovering and clicking a todo row must still resolve to the todo and show its tooltip.

### 3.6 The pinned test

`focus_walks_todo_then_subagent_then_job_rows_without_collapsing` (`sidebar.rs:2343-2372`) asserts the current order. It must be **updated to the new order**, not deleted and not weakened: the walk it protects — reaching a subagent row and then a job row without collapsing a section on the way — is still real.

### 3.7 A deliberate divergence from the `Ctrl+T` overlay

The overlay's `selectable_indices` (`src/tui/app/state/tasks.rs:402-408`) excludes headers — "a label, not something to open" — but deliberately **keeps** todo rows selectable (`tasks.rs:415`), because in the overlay a todo row *is* actionable: it carries the task detail.

The side panel's todo row has **no** action. The two surfaces therefore differ on purpose. This must be recorded as a decision, not "fixed" for consistency: the rule is *a focusable row has an action*, and the two surfaces have different actions.

## 4. Invariants that must not regress

- **I4 — Panel input authority.** Keys resolve through `SidebarActivation` → `PanelKey::Consumed` / `PanelKey::Intent(TuiMsg)` → `src/tui/app/input.rs`. `scripts/check_tui_transport_authority.py` must stay green.
- **I5 — A focusable row has an action.** (New; this milestone delivers it.)
- The **Todos section header stays focusable** and its activation stays `SidebarActivation::ToggleSection` (`sidebar.rs:621`), so Enter folds and unfolds the section. The user's requirement is to skip the *rows*, not the *header*.
- Collapsing a section must still hide its rows from the focus list (the existing `if !self.todos_collapsed` guard at `sidebar.rs:672`).
- `reconcile_focus` must continue to keep the focus valid when the row set changes underneath it (e.g. a todo completes and disappears).
- Mouse hover, tooltips, and click resolution on todo rows must keep working.

## 5. Scope

### In scope

- The side panel's **keyboard focus list** and the variants it implies.
- The hover→focus mapping for todo rows.
- The now-dead `ensure_focused_visible` arm.
- The pinned focus-walk test.

### Explicitly out of scope

- The `Ctrl+T` overlay's selection model (`src/tui/app/state/tasks.rs`). Its rule is different on purpose; do not touch it.
- The `/sessions` dialog's selection model.
- The rendered **content** of the side panel's rows — P002 owns the subagent row's shape.
- The mouse hit-test map's todo entries.
- Any change to what a todo row *displays*.

## 6. Required production changes

### Frontend — the focus list

1. **Todo rows leave `focus_targets()`.** Remove the `targets.extend(... SidebarFocusTarget::Todo)` from `focus_targets()` (`sidebar.rs:670-675`). Keep `targets.push(SidebarFocusTarget::Section(SidebarSection::Todos))` — the header stays focusable.

2. **Remove the `SidebarFocusTarget::Todo` variant.** With todo rows out of the focus list, the variant can no longer be produced by `focus_targets()`, and `move_focus` can never match it. Removing it — together with its arms in `activate_focused` (`sidebar.rs:640`), `ensure_focused_visible` (`sidebar.rs:504`), `hovered_to_focus` (`sidebar.rs:795`), and any other match — makes the model honest: a focus target is something that can be focused.

   `HoveredElement::Todo` **stays** — it is the mouse map, not the focus map.

3. **Fix the hover→focus mapping.** `hovered_to_focus` must map a hovered todo to the **Todos section header** (`SidebarFocusTarget::Section(SidebarSection::Todos)`) or to `None`. It must never produce a target that is absent from `focus_targets()`.

   Note the collapsed case: when the Todos section is collapsed its rows are not rendered, so a hovered todo cannot exist; the mapping is therefore only ever reached while the section is expanded, and the header is in the focus list in both states.

4. **Keep `activate_focused`'s Todos-header behaviour.** `SidebarFocusTarget::Section(_)` → `SidebarActivation::ToggleSection` (`sidebar.rs:621`) is unchanged.

### Tests

5. **Update** `focus_walks_todo_then_subagent_then_job_rows_without_collapsing` (`sidebar.rs:2343-2372`) to the new order: the walk goes Todos **header** → subagent rows → job rows, and no section collapses on the way. Keep every other assertion the test makes (that a subagent row and then a job row are reached, that nothing collapsed).

6. **Add** a test that a todo row is never the focused target after any sequence of `focus_next` / `focus_prev` calls.

7. **Add** a test that clicking (i.e. `focus_hovered`) a hovered todo does not clear the focus and does not cause the next `focus_next` to jump to index 0.

### Documentation and static guards

- No new static guard. This is a widget-internal focus model, and the evidence is unit tests.
- `scripts/check_tui_transport_authority.py` must stay green — no key binding changes.

## 7. Ordered work packages

### Work package A — The focus list

Intent: todo rows are not focus targets.

Required changes: `focus_targets()`; remove the `SidebarFocusTarget::Todo` variant and its arms.

Acceptance evidence: the updated focus-walk test; a new test asserting no todo row is ever focused.

### Work package B — The hover path

Intent: hovering and clicking a todo row does not corrupt the keyboard focus.

Required changes: `hovered_to_focus`'s todo arm.

Acceptance evidence: the new hover test — after `focus_hovered()` on a hovered todo, the focused target is still the header (or unchanged), and a subsequent `focus_next()` lands on the **next** focus target, not index 0.

### Work package C — The dead arm

Intent: no unreachable match arm remains.

Required changes: `ensure_focused_visible`'s match.

Acceptance evidence: `cargo clippy` clean (the compiler will flag an unreachable arm) plus the diff.

## 8. Failure, cancellation, restart, and contention semantics

- No state, async, or cancellation behaviour changes.
- `reconcile_focus` must still cope with the focus list changing underneath it — in particular when the last todo disappears while it is focused. With todo rows unfocusable, the focused target is a header or a row of another kind, and `reconcile_focus`'s fallback keeps the focus valid.
- A section that is collapsed while a row inside it is focused must still reconcile the focus to something present.

## 9. Compatibility and migration

None. No schema, protocol, configuration, or storage change. The change is visible only as a different `j`/`k` walk order.

## 10. Required tests

### Focused unit tests

- The updated `focus_walks_todo_then_subagent_then_job_rows_without_collapsing`.
- A test that no todo row is ever the focused target across a full forward and backward walk.
- A test that the Todos header **is** focusable and that `activate_focused()` on it returns `SidebarActivation::ToggleSection`.
- A test that clicking a hovered todo leaves a valid focus target and that the next `focus_next()` does not return index 0.
- A test that `line_targets()` still contains a `HoveredElement::Todo` for a rendered todo row (mouse map unchanged).

### Integration tests

- An integration test driving `j`/`k` in the session panel over a fixture with todos, subagents and jobs, asserting the sequence of focused identities.

### Negative tests

- With **no** todos, the focus list is unchanged from today's for the other sections.

### Security and negative tests

None specific.

## 11. Required verification commands

```bash
# narrow tests first
cargo test -p codegg --lib tui::components::sidebar
cargo test -p codegg --test tui_side_panels

# static guards
python3 scripts/check_tui_transport_authority.py

# formatting and linting
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings

# broader suite appropriate to the change
scripts/verify.sh quick
```

Do not claim any command that was not actually run.

## 12. Documentation updates

- `docs/tui.md` §"Sessions" / the panel description — state that `j`/`k`/arrows skip rows with no action and that the "Todos" header folds the section.
- `architecture/tui.md` — the side-panel focus model; and the "one row, five surfaces" paragraph if it describes the side panel's focus behavior.

## 13. Acceptance criteria

1. `j`/`k`/arrows in the `SESSIONS` side panel never land on a todo row.
2. The **Todos** header remains focusable, and Enter on it collapses/expands the section.
3. The selection highlight is never painted on a todo row.
4. Clicking a todo row does not clear the keyboard focus and does not cause the next navigation key to jump to the top of the panel.
5. Mouse hover and tooltips on todo rows still work; `line_targets()` is unchanged.
6. The `Ctrl+T` overlay's selection model is **untouched**.
7. `scripts/verify.sh quick` is green.

## 14. Stop conditions

Stop and report rather than improvise when:

- **P002 is not closed.** This milestone's dependency is hard.
- Removing the `Todo` focus variant would require changing the mouse hit-test map — it must not; report the conflict instead.
- The `Ctrl+T` overlay appears to need the same change. It does not; its rule is different on purpose.
- The focus model appears to require a new key binding or a change to `SidebarActivation` — out of scope; report it.

## 15. Closure evidence required

- The implementation SHA.
- The requirement-to-evidence matrix over the seven acceptance criteria.
- The exact commands run and their results, including `scripts/verify.sh quick`.
- The updated focus-walk test's **old and new** expected orders, quoted, so the change is legible without a diff.
- An explicit statement of what was **not** run.

## 16. Handoff notes

- The user's request is precise: skip the **rows**, keep the **header**. Both halves matter; "simplify navigation" is not the requirement.
- The two pinned tests in `sidebar.rs` — `a_subagent_row_shows_its_descriptor_when_the_column_has_room` and `a_subagent_row_drops_the_descriptor_when_the_column_is_too_narrow` — were changed by **P002**, not by this milestone. If they are still failing, P002 has not landed.
- The mouse map and the keyboard map are different things. `line_targets()` is the mouse map; `focus_targets()` is the keyboard map. Conflating them is how the current bug came to look like a rendering bug.
