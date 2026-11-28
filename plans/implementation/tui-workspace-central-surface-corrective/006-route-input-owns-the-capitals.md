# TUI Workspace Central Surface Corrective Milestone 006 — A focused route input owns the capitals

Status: ready for handoff

Repository baseline: `256b215ac` on `feat/auto-20261126-cae46b00` (worktree clean; verification performed 2026-11-27 against this exact commit)

Source roadmap:

- `plans/subsystems/tui-workspace-central-surface-corrective-addendum.md#13-post-closure-verification-and-corrective-milestones`
- `plans/subsystems/tui-workspace-central-surface-corrective-addendum.md#m001--the-workspace-route-owns-a-composer-and-stops-swallowing-composition-keys`

Source closure record this milestone corrects:

- `plans/closure/tui-workspace-central-surface-corrective/001-status.md` — records M001 closed on a filtered run (`cargo test -p codegg-tui --test tui_render -- workspace`, 19 passed). The filter cannot observe this defect.

Long-term requirements:

- `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`
- `plans/000-long-term-specification.md#25-tui-target-behavior` — "Exact keys remain configurable. The conceptual hierarchy and read-only observation behavior are normative." The section does **not** settle which surface owns a shifted letter, so the ADR below does.
- `plans/003-planning-process.md#7-corrective-passes` — this is a corrective plan, not an amendment.

Applicable ADRs:

- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` — the accepted rule this milestone restores: in `Insert` characters are text, in `Normal` bare characters are claimable app actions, and "a claimed key must not silently fall through to an unrelated app verb when the composer's own" surface owns it (`:62-70`, `:90`). The route's early arms bypass that rule entirely.
- `plans/adrs/ADR-0052-cross-frontend-command-execution-ownership.md` — no command catalog change.

Primary class: **invariant**. The capability exists; its key-ownership boundary is wrong.

## 1. Objective

While a text surface inside `/workspace` owns the keyboard (route composer, Chat composer, member form, launch branch name, filter), a shifted printable reaches that surface and **no route verb fires**; from the Normal tree those verbs keep their meaning.

## 2. Why this milestone is ready

The defect is reproduced live and in source, and its resolution is already decided by an accepted ADR:

- `handle_workspace_view_key` (`crates/codegg-tui/src/commands/workspace_view.rs:937`) arms `(Char('X'), SHIFT)`, `(Char('H'), SHIFT)`, `(Char('L'), SHIFT)` at `:948-958` and returns, **before** the Panel/Composer branch at `:1000-1009` that would otherwise hand the key back to the app.
- Live receipt (isolated `HOME`/`CODEGG_DATA_HOME`, 150×44, cwd `/tmp/wsverify/repo`, session `wsverify`): with the route composer focused (badge `[INS][CHAT]`, placeholder "Message the project chat…"), typing `Hi` left only `i`; a capital `L` switched the title to `NOR · chat panel`; a capital `X` left the route and stranded the orphaned `i` in the session composer, where it becomes an LLM turn.
- ADR-0042 outranks `architecture/tui.md`'s sentence "`Shift+X` leaves the route from any Workspace focus" (`:3288-3296`). That sentence is the only place the current behaviour is documented, and the same paragraph already says "composition keys otherwise fall through to the selected input" — the two halves contradict each other and the code follows the wrong one.

No dependency is open. Nothing here needs a daemon, protocol, storage, or config change.

## 3. Current implementation evidence

### 3.1 Precedence inside the handler

Order today (`:937-1186`): no-dashboard guard → `Shift+X/H/L` → `Shift+F`/`Ctrl+\` decline → tree-only panel-manager decline → inline project-picker phase → member form → launch sheet → Panel/Composer (Esc only; everything else `false`) → Insert/filter → Normal tree.

The three capital arms therefore precede every text-owning state. Member forms and the launch branch step already accept shifted characters (`:771-780`, `:918-928`), so they are only saved by being unreachable for those three letters.

### 3.2 Focus state

`WorkspaceInputFocus` is the local authority for the handler; `App`'s routing and the composer badge read `workspace_focus` plus the chat seed/panel host. The setters agree (`focus_workspace_composer`, `focus_workspace_chat` at `workspace_dashboard.rs:721-767`; `open_workspace_dashboard_inner` parks the composer; `leave_workspace_view` clears the binding), so this milestone changes **precedence**, not state.

### 3.3 Test surface

- `workspace_focus_routes_composition_once_and_restores_the_session_mode` (`workspace_dashboard.rs:1193-1255`) focuses the composer and presses lowercase `n` only. No route test presses a capital while composing.
- `builtin_shift_letters_are_composed_text_in_insert_mode` (`input.rs:3014-3057`) sweeps the keymaps, not this route; the editor has its own route-level sweep (`commands/editor.rs:9382-9476`). Neither reaches `/workspace`.
- `crates/codegg-tui/tests/tui_render/tui_composer_claims.inc:821-854` was amended in the same commit to press `i` before `d`. Its doc comment still claims "in Normal mode an unbound letter is filter text", which the change made false: Normal-mode `d` is the detail verb and filter text requires `i` (`:1097-1101`, `:1140-1143`).

## 4. Invariants that must not regress

- A typed printable reaches exactly one visible input, and that input names its destination first (M001's own contract).
- From Normal tree focus, `Shift+X` leaves, `Shift+H` focuses the composer, `Shift+L` focuses Chat, `Shift+F`/`Ctrl+\` reach the global picker, and `Esc` keeps its cleanup ladder.
- Composer `Esc` leaves the route; tree `Esc` closes detail/reveal/filter before exit (`:1000-1009`, `:1061-1095`).
- No centered small box is introduced for any workspace action.
- No second key table: the fix moves conditions, it does not add a parallel claim list.
- **The eight `tui_render` failures present at the baseline are pre-existing and out of scope.** Measured at `256b215ac`: `invariants::baseline`, `invariants::vocabulary`, four `tui_panel_docking` members, `tui_panel_host_chrome::task_view_advertises_only_the_verbs_a_column_can_deliver`, and `tui_side_panels::walking_the_panel_list_loads_the_panel_it_lands_on`. They fail identically at `eefbfa0fa`. Do not fix them here and do not attribute them to this milestone.

## 5. Scope

### In scope

- Gate `Shift+X`, `Shift+H`, `Shift+L` on `focus == WorkspaceInputFocus::Tree && mode == WorkspaceMode::Normal`, placed **after** the inline-picker, member-form, launch-sheet and Panel/Composer branches.
- Route-local regression tests, including the composer half and a vacuity check.
- Correct the stale doc comment in `tui_composer_claims.inc` and the contradictory `architecture/tui.md` sentence.
- Update `docs/tui.md`'s route key list if it states "from any focus".

### Explicitly out of scope

- Filter-mode `Enter`/`Tab` and the title-row vocabulary (M007).
- Chat binding and history (M007); sessionless team scope (M008); lifecycle and launch (M009); `/worktree` (M010).
- Changing ADR-0042's claim table, the global binding maps, or the composer's Normal-mode letters.
- The pre-existing eight failures.

## 6. Required production changes

### Frontend (the only production surface)

Reorder and gate `handle_workspace_view_key`. The three arms move below the picker/member/launch/focus branches and gain the tree/Normal condition. Keep them ahead of the tree commands so `Shift+X` still beats the `Char('x')` destroy arm, which stays a separate lowercase binding. `Shift+F` and `Ctrl+\` keep declining to the app (they already do, `:960-963`) so the global picker stays reachable from the tree and composes as text elsewhere.

Do not add a "detail open" exclusion: `detail_open` changes `Esc`, not the keyboard owner (`:943-944`, `:1075-1095`).

### Documentation and static guards

- `architecture/tui.md:3288-3296` — replace "leaves the route from any Workspace focus" with the truthful rule (tree focus; `Esc` from the composer), keeping the existing "composition keys otherwise fall through" sentence, which becomes true.
- `docs/tui.md:129-150` — align the key list with the corrected rule.
- No script guard constrains this precedence. `check_command_catalog_ownership.py`, `check_frontend_action_set.py` and `check_tui_transport_authority.py` are unaffected and must stay green.

## 7. Ordered work packages

### Work package A — Move and gate the capital verbs

Intent: the three route verbs become tree-only.

Required changes: lift the arms to after the inline-picker/member/launch/Panel/Composer branches; condition them on `focus == Tree && mode == Normal`; keep `Shift+F`/`Ctrl+\` where they are.

Acceptance evidence: pressing `Shift+X/H/L` while composer-focused composes; from the tree it performs the verb; `Shift+L` from composer focus types `L`.

### Work package B — Route-local regression tests

Intent: the defect cannot return without a red test.

Required changes: add behavioural tests through `App::on_key` (not the handler in isolation) in `commands/workspace_dashboard.rs` beside the existing route-key tests:

1. `the_route_composer_owns_shifted_letters` — open a populated Workspace, focus the composer, press `X`, `H`, `L`, `F`; assert the exact prompt text, that the route is still open, and that focus is unchanged.
2. `shift_x_leaves_only_from_the_normal_tree` — from the tree, `Shift+X` leaves; from the composer it does not.
3. A vacuity control in each: a tree-owned key (`d`) still opens detail, proving the route handler was reached.

Acceptance evidence: both tests fail on `256b215ac` and pass after the fix.

### Work package C — Evidence and prose

Intent: the record stops describing the old contract.

Required changes: correct the `tui_composer_claims.inc` doc comment (and rename the test only if its name now lies); update `architecture/tui.md` and `docs/tui.md`.

Acceptance evidence: no document states that `Shift+X` exits while a text input owns the keyboard.

## 8. Failure, cancellation, restart, and contention semantics

Key routing is synchronous and stateless; no restart or contention path is introduced. Two edges must be preserved: the inline picker phase consumes keys before any route verb (`:977-987`), and the member form and launch branch step keep accepting shifted characters as text.

## 9. Compatibility and migration

No storage, protocol, config or keymap change. A user binding on `Shift+X` outside the route is unaffected, and inside the route the tree still wins as before.

## 10. Required tests

### Focused unit tests

`cargo test -p codegg-tui --lib workspace_dashboard` — the existing route-key tests plus the two new ones.

### Integration tests

`cargo test -p codegg-tui --test tui_render` — run **unfiltered**. The only acceptable result is the eight pre-existing failures; a ninth is this milestone's.

## 11. Required verification commands

```bash
cargo test -p codegg-tui --lib workspace_dashboard
cargo test -p codegg-tui --test tui_render          # unfiltered; eight known reds only
scripts/verify.sh quick
scripts/verify.sh scoped 256b215ac
```

Live receipt (isolated environment, exact recipe in addendum §13.2): focus the route composer, type `Hi There X`, assert the full string in the prompt and the route still active; `Esc`; from the tree press `Shift+X` and assert the route closed and the prior tab restored.

## 12. Documentation updates

`architecture/tui.md` (route mode/key paragraph), `docs/tui.md` (route key list). No help-overlay row changes.

## 13. Acceptance criteria

- A capital typed into the focused route composer appears in the prompt, and the route does not change focus or exit.
- `Shift+X/H/L` from Normal tree focus behave exactly as today.
- `Esc` semantics are unchanged in both focuses.
- The unfiltered `tui_render` run shows no failure beyond the eight recorded pre-existing ones.
- A live `tui-test` receipt records the composer half and the tree half.

## 14. Stop conditions

- Stop if the fix would require editing ADR-0042's claim table, the global binding maps, or the composer's Normal-mode letters.
- Stop and report if a product decision wants `Shift+X` to exit **while** the composer owns the keyboard: that reverses ADR-0042's rule and needs an owner decision plus an ADR amendment, not a code change.

## 15. Closure evidence required

`plans/closure/tui-workspace-central-surface-corrective/006-status.md` with: the unfiltered family run (counts and the named pre-existing failures), the two new test names and their before/after result, the receipt path for the live drive, and the documentation diff summary.

## 16. Handoff notes

- The handler is one function; the whole change is precedence plus a condition. Read `:937-1010` before editing.
- Do not "fix" this by removing the capital arms: the tree must keep them, and ADR-0042 forbids a composer claim for them.
- The live harness recipe (session creation, isolated env, exact commands) is in the addendum §13.2 so it does not have to be rediscovered.
