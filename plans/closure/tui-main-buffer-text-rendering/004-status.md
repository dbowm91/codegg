# TUI Main Buffer Text Rendering Milestone M004 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/tui-main-buffer-text-rendering/004-row-precise-disclosure-and-expand-all.md`

Source subsystem roadmap:

- `plans/subsystems/tui-main-buffer-text-rendering-roadmap.md#milestone-m004--row-precise-disclosure-and-expand-all`

Repository baseline reviewed: `f2639b5d2`

Implementation commits or pull requests:

- **this commit** — the implementation and this closure record land together
  in one commit on `chore/record-landing-lane-outcomes` (baseline
  `f2639b5d2`). The landing note appended after the merge records that
  commit's SHA and the hosted `CI / verify` result for it; see §10.
  The files that carry this milestone's change are
  `src/tui/components/messages/layout.rs` (the part boundaries and the
  row→part resolution), `src/tui/components/messages.rs` (the per-part
  disclosure state and the producer's `PartSpan` records),
  `src/tui/app/mod.rs` (the click handler and the expand-all handler),
  `src/tui/input.rs` (the action, the `ActionKey`, the chord and the
  two-map binding), `src/tui/command.rs` (the `/expand-all` palette
  command), `scripts/check_tui_composer_key_authority.py` (the chord-only
  action and the repaired brace matcher),
  `scripts/check_tui_editor_key_authority.py` (the corrected prose counts),
  `tests/tui_render/tui_main_buffer_row_precise_disclosure.inc` (the
  integration member), `architecture/tui.md`, `docs/tui.md`,
  `architecture/command.md` and `.opencode/skills/tui/SKILL.md`.
  Several of those files are shared with this line's other three
  milestones; the ones no other milestone touches are
  `src/tui/components/messages/layout.rs`,
  `tests/tui_render/tui_main_buffer_row_precise_disclosure.inc` and
  `scripts/check_tui_editor_key_authority.py`. No SHA, pull request
  number or commit message is claimed, because none exists: the
  implementing agents were not authorized to commit.

## 1. Executive finding

The milestone's capability boundary — **does a click expand the thing
under the cursor, and is there one gesture that opens the whole
transcript** — is complete in the working tree, and the completion is
verified by test rather than by assertion.

At the baseline the click target was the **message**, and the handler
asked "does this message have reasoning?" **before** "does it have tool
output?". The reasoning test was true whenever the message carried
**any** reasoning part, expanded or not. So in a message carrying both
a reasoning block and a tool call, click 1 expanded the reasoning, click
2 collapsed it again, and the tool call was **never reachable by
clicking at all**. The milestone replaces the message-level unit with a
**part-level** one: the row producer records which rows belong to which
part, a clicked row resolves to the part that owns it, and the handler
toggles **that** part. A row that owns nothing expandable toggles
**nothing** — an explicit no-op, not a fallback to the message-level
toggle, which would have reintroduced the defect the milestone exists to
fix. The aggregate gesture lands with it: one action expands every tool
call and every reasoning block in the transcript, a second press
collapses every one, and the command palette carries the same gesture
under `/expand-all`.

What landed:

- `src/tui/components/messages/layout.rs` — `RowPart { ToolCall { part_idx },
  Reasoning { part_idx } }`, `PartSpan { part, start, count }`, the
  cache's `part_offsets: Vec<(usize, RowPart, usize, usize)>` and
  `find_part_at_line`.
- `src/tui/components/messages.rs` — the producer's `PartSpan` records,
  `MessagesWidget::part_at_viewport_line`, `toggle_tool_call_part`,
  `toggle_reasoning_part`, `any_disclosure_collapsed` and
  `set_all_disclosures_expanded`.
- `src/tui/app/mod.rs` — the click handler resolves the **part**; the
  expand-all handler and its toast.
- `src/tui/input.rs` — `InputAction::ExpandAllDisclosures` and
  `ActionKey::ExpandAllDisclosures`, bound in **both** maps.
- `src/tui/command.rs` — `BuiltinSlashAction::ExpandAllDisclosures` and
  the `/expand-all` palette command.
- The two key-authority guards — **repaired**, not extended.

No protocol, storage, migration, authorization, or dependency change.
No closed milestone of any other roadmap was reopened or amended.

### The owner's chosen chord was not deliverable, and shipped differently

**The owner chose `Ctrl+Shift+O`. It shipped as `Ctrl+Alt+O`.** This is
a **deviation from the owner's stated choice**, and it is recorded as
one — in §3 (what changed), §8 (the interaction contract) and §10 (a
named finding with the action the owner must take). It is not a
correction the milestone made on its own authority.

The reason is the platform, and it was verified against the source
before the substitution was accepted:

- The app **never pushes keyboard-enhancement flags**. `src/tui/terminal.rs`
  enables raw mode (`:72`), bracketed paste (`:80`) and mouse capture
  (`:88`) only; `PushKeyboardEnhancementFlags` appears nowhere in `src/`
  or `crates/`. The terminal is therefore in **legacy xterm key
  encoding**, where a `Ctrl+Shift+O` keystroke arrives
  **indistinguishable from `Ctrl+O`** — it would have fired the
  workspace dashboard instead of the gesture.
- The repository **already forbids** a `CONTROL|SHIFT` chord in either
  binding map:
  `tests/tui_render/modifier_chords.inc::no_binding_map_contains_a_control_shift_entry`
  is a structural assertion over the parsed maps that fails if either
  map binds `CONTROL|SHIFT`. The plan's §2 had assumed `Ctrl+Shift` was
  "free"; it is free of bindings but not deliverable.
- The contract is already written down in three places:
  `architecture/tui.md` (the "Chord contract" section — "Multi-modifier
  chords are `{CONTROL, ALT}` only"), `docs/tui.md` ("Keybindings with two
  modifiers use `Ctrl+Alt`, never `Ctrl+Shift`") and
  `src/tui/input.rs:966`.

`Ctrl+Alt+O` arrives as ESC plus one control byte, which crossterm
reports as `CONTROL|ALT` — unambiguous against both `Ctrl+O` and bare
`Alt+O`. `/expand-all` sits beside it so the capability is reachable
without knowing the chord at all. **`Ctrl+O` was not re-spelled**: the
workspace dashboard keeps it in **both** maps.

### The condition, the risk, and the exact future evidence

**The condition.** The implementation is complete and locally verified,
and this change carries it in a commit — but the **hosted `CI / verify`
lane has not run against that commit**, and a CI run cannot precede
the commit it runs against. The exact future evidence that discharges
the condition is:

1. a **green hosted `CI / verify` run against that commit on the hosted
   runner** — not the local gate, and not a re-run of the local gate. The exact future evidence that discharges the
condition is:

1. a **green hosted `CI / verify` run against that commit on the hosted
   runner** — not the local gate, and not a re-run of the local gate.

**The risk.** A local `scripts/verify.sh quick` run is a property of
**this machine and this tree**, not of the hosted runner. The hosted
lane is a strict **subset** of `quick` — it omits the client and desktop
boundaries and the provider wire/catalog/resilience, OpenAI endpoint and
Eggwork routing guards — so a green hosted run does **not** by itself
prove everything the local gate proves. The local gate's own value is
what §4 states: it is strong evidence about this tree, and it is **not**
evidence about the hosted runner's environment, its runner image, or its
concurrency.

A second, smaller risk rides on the same condition: the chord the
owner asked for is not the chord that shipped, and the owner's
ratification of `Ctrl+Alt+O` is outstanding (§10).

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| §13.1 — clicking a tool-call header expands **that** tool call, in a message that also carries a reasoning block | `clicking_a_tool_call_row_expands_the_tool_call_and_not_the_reasoning` (`src/tui/app/mod.rs:20521`); `clicking_each_header_expands_that_part_independently` (`tests/tui_render/tui_main_buffer_row_precise_disclosure.inc:204`) | pass | The integration member asserts the **other** part stays closed, which is the baseline defect. |
| §13.2 — clicking a reasoning header expands **that** reasoning block | `clicking_a_reasoning_row_expands_the_reasoning_block` (`src/tui/app/mod.rs:20540`); the same integration member | pass | The reasoning block is now reachable **only** by clicking a reasoning row. |
| §13.3 — clicking a row that holds nothing expandable changes nothing | `clicking_a_prose_row_toggles_nothing` (`src/tui/app/mod.rs:20561`); `clicking_a_prose_row_changes_nothing` (`tests/tui_render/tui_main_buffer_row_precise_disclosure.inc:241`) | pass | The `None` arm is an **explicit no-op** in the handler (`src/tui/app/mod.rs:9305`), not a fallback to the message-level toggle. |
| §13.4 — one gesture expands every hidden detail; a second press collapses every one; the palette command does the same; the toast says which happened | `the_expand_all_gesture_opens_everything_and_the_next_press_closes_it` (`src/tui/app/mod.rs:20578`); `the_palette_command_expands_everything_too` (`:20607`); `the_palette_carries_the_expand_all_command` (`:20627`); `expanding_every_disclosure_opens_every_part_and_closes_them_again` (`src/tui/components/messages.rs:6029`) | pass | The toast is `"all details expanded"` / `"all details collapsed"` (`src/tui/app/mod.rs:11003`, `:11005`). The palette command dispatches to the **same** handler (`:6575`), so the toast is identical by construction. |
| §13.4 — **the chord is `Ctrl+Shift+O`** | `the_expand_all_chord_is_bound_in_both_maps` (`src/tui/input.rs:3590`) binds `(CONTROL \| ALT, Char('o'))`; `no_map_binds_a_control_shift_chord` (`:3616`) | **partial** | **The requirement is met in behaviour and not in spelling.** `Ctrl+Shift+O` is not deliverable in this TUI (legacy xterm encoding — §1), so the chord shipped as **`Ctrl+Alt+O`**. This is a **deviation from the owner's stated choice** and it is a named finding in §10, not a silent correction. |
| §13.5 — `Ctrl+O` still opens the workspace dashboard in both binding maps | `the_workspace_dashboard_keeps_ctrl_o_in_both_maps` (`src/tui/input.rs:3635`) | pass | The new chord is additive; it does not take the dashboard's key. |
| §13.6 — the key-authority guards pass, with their recorded chord counts updated, and their self-tests still reject their defect classes | §4's guard table: both guards pass, both `--self-test`s pass, and the editor guard's prose counts are corrected against its own summary line (66 → **72** Normal-mode chords; "three" → **two** guard-block chords) | pass | The composer guard's chord model is now **five** chord-bound actions — the four that moved off a claimed letter plus the one that was **born** as a chord. |
| §15 — the row→part resolution's and the click handler's tests | `a_row_resolves_to_the_part_that_owns_it` (`src/tui/components/messages.rs:5898`); `a_rendered_row_resolves_to_the_part_that_owns_it` (`tests/tui_render/tui_main_buffer_row_precise_disclosure.inc:177`); `toggling_one_part_leaves_the_message_s_other_parts_alone` (`src/tui/components/messages.rs:5974`); `toggling_a_part_that_does_not_exist_changes_nothing` (`:6009`) | pass | A prose row resolves to **no** part, and the toggle of a non-existent part changes nothing. |
| §15 — the expand-all's tests, including the palette command and the toast | `expanding_every_disclosure_does_not_touch_the_prose` (`src/tui/components/messages.rs:6066`); the four app-level tests in `src/tui/app/mod.rs` | pass | The prose is untouched: model prose is not a disclosure. |
| §15 — the binding-map assertion for the chord and for the unchanged `Ctrl+O` | `the_expand_all_chord_is_bound_in_both_maps`, `no_map_binds_a_control_shift_chord`, `the_workspace_dashboard_keeps_ctrl_o_in_both_maps` (`src/tui/input.rs:3590`, `:3616`, `:3635`); `no_binding_map_contains_a_control_shift_entry` (`tests/tui_render/modifier_chords.inc`) | pass | The last one is the **structural** assertion over the parsed maps that forbids `CONTROL\|SHIFT` in either map. |
| §15 — the guards' output and their self-tests' results, with the old and new chord counts stated | §4's guard table: editor guard reports "**2** guard-block chord(s), **72** Normal-mode chord(s), **8** Insert-mode chord(s)"; the prose said 66 and "three" before the repair | pass | The counts are stated against the guard's **own** summary line, which is the source of truth. |
| §15 — the scrollbar's row-count assertion after an expand-all (I1) | `the_scrollbar_follows_the_row_count_after_an_expand_all` (`tests/tui_render/tui_main_buffer_row_precise_disclosure.inc:265`) | pass | The assertion is that the **drawn** rows equal the **counted** rows after the expand, and that the scroll range comes from the same count. |
| §15 — `scripts/verify.sh quick` green at the closing commit | §4's gate table: **73 of 73 steps green**, "Quick verification passed", exit 0 — in the **working tree** | **not met** | The gate is green, but **the closing commit does not exist yet** and the hosted lane has not run. This is the milestone's condition; see §1 and §11. |
| The gesture is discoverable without the chord | `Command::new("/expand-all", …)` with aliases `expand-disclosures` and `collapse-all` (`src/tui/command.rs:600`); `BuiltinSlashAction::ExpandAllDisclosures` (`:189`) | pass | The built-in command count moved 168 → **169**, and `architecture/command.md` now states 169 and 161 `BuiltinSlashAction` variants. |
| The action is user-rebindable and appears in the keybind editor | `ActionKey::ExpandAllDisclosures` (`src/tui/input.rs:327`), listed in `ActionKey::all` (`:398`) and labelled "Expand or collapse every detail (Ctrl+Alt+O)" (`:609`) | pass | `InputAction` ↔ `ActionKey` conversion is total in both directions (`:464`, `:543`). |
| A pending call's **status row** resolves to the tool call | The status row is emitted inside the tool-call arm, so the arm's `PartSpan` covers it (`src/tui/components/messages.rs:2200`, `:2463`) | pass | The status row is the backup way back into a dismissed approval, so it must resolve to the call it belongs to. |
| The model-facing path was not touched (I3) | `git diff f2639b5d2 -- src/search_backend/` is **zero lines**; `src/search_backend/framing.rs` has **no diff lines** | pass | The trust frame reaches the model byte for byte. |
| The whole `--lib` suite | §4's "the full `--lib` run" table: **7371 passed, 5 failed**, 4 ignored | **partial** | The five failures are **environmental or load-sensitive**, none is this milestone's; each is attributed in §4. |
| Degenerate inputs resolve to nothing rather than to a part | `find_part_at_line` returns `None` for `target >= self.total_lines` and for a row no entry covers (`src/tui/components/messages/layout.rs:204`) | pass | A row past the end of the buffer, a prose row, a timestamp and a separator all resolve to `None`. |

## 3. Production implementation evidence

### The part boundaries — `src/tui/components/messages/layout.rs`

```rust
pub enum RowPart {
    ToolCall { part_idx: usize },
    Reasoning { part_idx: usize },
}

pub struct PartSpan {
    pub part: RowPart,
    pub start: usize,
    pub count: usize,
}
```

The layout cache gains `part_offsets: Vec<(usize, RowPart, usize, usize)>`
— `(msg_idx, part, first row index, row count)` per expandable part, in
row order. The **one** row producer records the runs:
`build_assistant_parts_lines` pushes a `PartSpan` at the end of the
reasoning arm (`:2182`) and at the end of the tool-call arm (`:2463`),
and `push_message` translates the spans through the blank collapse, so a
part's recorded range is the range of what is **drawn** and not of what
was emitted. A part whose every row was dropped by the collapse owns
nothing that can be clicked and is skipped.

`find_part_at_line` binary-searches `part_offsets` exactly as
`find_message_at_line` does `message_offsets`. The message-level
resolution is **untouched** and still answers "which message is this row
in"; the new resolution answers "what does this row hold".

### The resolution entry point — `src/tui/components/messages.rs`

`MessagesWidget::part_at_viewport_line(line)` (`:1515`) →
`cache.find_part_at_line(scroll_position() + line)` →
`Option<(msg_idx, RowPart)>`.

### The disclosure state — `src/tui/components/messages.rs`

- `toggle_tool_call_part(msg_idx, part_idx)` (`:1146`) and
  `toggle_reasoning_part(msg_idx, part_idx)` (`:1162`) — the unit is the
  **part**, not the message; each invalidates the rows from its message.
- `any_disclosure_collapsed()` (`:1180`) — only a disclosure that has
  something to show counts: a tool call with no output and a reasoning
  block with no text are not collapsed things.
- `set_all_disclosures_expanded(expanded)` (`:1197`) — folds every
  message's parts, sets `ToolCall { expanded }` and
  `Reasoning { collapsed }` accordingly, counts what changed, and
  invalidates the rows from 0 when anything changed. **Model prose is
  not a disclosure and is not touched.**

### The click handler — `src/tui/app/mod.rs`

`dispatch_click` → `on_click`, the viewport arm (`:9271`), resolves the
**part** that owns the clicked row and toggles **that** part:

- a row that belongs to a tool call toggles the tool call (`:9290`);
- a row that belongs to a reasoning block toggles the reasoning block
  (`:9295`);
- a row that belongs to **neither** toggles **nothing** — an explicit
  no-op (`:9305`), **not** a fallback to the message-level toggle.

The old ordering — which asked "does this message have reasoning?"
**before** "does it have tool output?", so that in a message carrying
both the second click collapsed the reasoning again and the tool call
was never reachable — is **gone**. The reasoning block is now reachable
only by clicking a reasoning row. A row waiting on an approval is still
an **action** first (`:9286`); a pending call's **status row** resolves
to the tool call.

### The expand-all gesture — `src/tui/app/mod.rs`, `src/tui/input.rs`, `src/tui/command.rs`

- The action is `InputAction::ExpandAllDisclosures`
  (`src/tui/input.rs:215`), and it also exists as
  `ActionKey::ExpandAllDisclosures` (`:327`), so it is user-rebindable
  and appears in the keybind editor.
- It is bound in **both** binding maps to **`Ctrl+Alt+O`** —
  `(KeyModifiers::CONTROL | KeyModifiers::ALT, KeyCode::Char('o'))`
  (`src/tui/input.rs:1197` for the default map, `:1451` for the vim map).
  **This is the substitution of §1 and §10**: the owner chose
  `Ctrl+Shift+O`, which is not deliverable in this TUI.
- The palette command is **`/expand-all`**, with the aliases
  `expand-disclosures` and `collapse-all`
  (`src/tui/command.rs:600`). It dispatches to the **same** handler as
  the chord (`src/tui/app/mod.rs:6575`), so the toast is identical.
- The toast reads **"all details expanded"** / **"all details
  collapsed"** (`src/tui/app/mod.rs:11003`, `:11005`).
- Expansion covers every tool call and every reasoning block. **Model
  prose is untouched** — it is never collapsed, so there is nothing to
  expand.
- **`Ctrl+O` still opens the workspace dashboard in both maps.** It was
  **not** re-spelled.

### The guard repairs — repaired, not extended

- `scripts/check_tui_composer_key_authority.py` now models **five**
  chord-bound actions — the four that *moved* off a claimed letter
  (`RESPELLED_ACTIONS`) plus the one that was **born** as a chord
  (`CHORD_ONLY_ACTIONS = { "ExpandAllDisclosures": "o" }`) — and polices
  the new chord-only action with its **existing** rules: bound in both
  maps, reachable by a chord and never by the bare claimed letter (`o`
  is the composer's), and documented under its modifier chord. The
  guard's docstring and rule text were widened from "re-spelled" to
  "chord-bound" so the rule is stated for what it now covers.
- Its **brace matcher was repaired**: `_matching_brace` previously
  stopped at a `}` inside a comment, so it had never seen the second
  half of the vim map. It is now comment- and literal-aware (line
  comments, block comments, string literals, character literals and
  lifetimes).
- `scripts/check_tui_editor_key_authority.py` had two **stale counts in
  prose** (66 → **72** Normal-mode chords; "three" → **two** guard-block
  chords), corrected against the guard's own summary line.
- **No new static guard was added.**

### Tests

- `src/tui/components/messages.rs` — 5 M004 unit tests: the row→part
  resolution, the per-part toggle, the toggle of a part that does not
  exist, the expand-all round trip, and the prose being untouched.
- `src/tui/app/mod.rs` — 6 M004 unit tests: three click cases, the
  expand-all round trip, the palette twin, and the palette carrying
  the command.
- `src/tui/input.rs` — 3 M004 unit tests: the chord in both maps, no
  `CONTROL|SHIFT` chord in either map, and `Ctrl+O` still on the
  dashboard.
- `tests/tui_render/tui_main_buffer_row_precise_disclosure.inc` — a
  **new** integration member with 4 tests: the rendered row's part
  resolution, each header clicked independently, a prose row changing
  nothing, and the scrollbar following the row count after an
  expand-all.

**Non-vacuity.** Every new test fails against the baseline tree: at the
baseline a click on a tool-call row in a message that also carries
reasoning expands the **reasoning**, and no expand-all gesture exists
at all. The `no_map_binds_a_control_shift_chord` test in
`src/tui/input.rs` and `no_binding_map_contains_a_control_shift_entry`
in `tests/tui_render/modifier_chords.inc` are the structural guards
against the substitution's cause returning.

### Documentation

- `architecture/tui.md` — "A row resolves to its part, not to its
  message" and "The expand-all gesture": the part boundaries, the
  resolution path, the three click outcomes, the chord and **why it is
  `Ctrl+Alt` and not `Ctrl+Shift`**, the palette command, the toast, and
  the guard repairs.
- `docs/tui.md` — "Opening everything at once": the `Ctrl+Alt+O` chord,
  the palette aliases, and the two toast strings.
- `architecture/command.md` — the `/expand-all` row with its aliases,
  and the corrected built-in counts (169 commands, 161
  `BuiltinSlashAction` variants).
- `.opencode/skills/tui/SKILL.md` — the gesture, the substitution and
  its reason, and the composer guard's five-chord model.
- `architecture/overview.md` — the integration-test-file count row
  (**327**), which this milestone's new member moved.

## 4. Verification executed

### Commands run

Every command below was run by **this record's author** on this
working tree. The evidence digest for this milestone did not carry the
implementing agent's per-command results, so §4 reports the run that
actually happened here rather than reproducing a table the digest does
not contain.

```bash
# the disclosure model and the new action, narrowly
cargo test --locked -p codegg --lib tui::components::messages
cargo test --locked -p codegg --lib tui::input

# the rendered transcript and the click path
cargo test --locked -p codegg --test tui_render

# the whole lib suite, to surface the environmental failures
cargo test --locked -p codegg --lib

# the key-authority guards, repaired and re-counted
python3 scripts/check_tui_composer_key_authority.py
python3 scripts/check_tui_composer_key_authority.py --self-test
python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_tui_editor_key_authority.py --self-test

# static guards that cover the new tests
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py

# formatting and the workspace check
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked

# the routine gate
bash scripts/verify.sh quick
```

### Results

| Command | Result |
|---|---|
| `cargo test --locked -p codegg --lib tui::components::messages` | **80 passed**, 0 failed |
| `cargo test --locked -p codegg --lib tui::input` | **54 passed**, 0 failed |
| `cargo test --locked -p codegg --test tui_render` | **1052 passed**, 0 failed |
| `cargo test --locked -p codegg --lib` (whole suite) | **7371 passed, 5 failed**, 4 ignored, 0 measured — see the failure table below |
| `python3 scripts/check_tui_composer_key_authority.py` | pass — "the claim table is closed, single-owner, leaks no key to an app-level action, and every caret motion is bound the same way in both maps" |
| `python3 scripts/check_tui_composer_key_authority.py --self-test` | pass — **13 checks**: twelve violation classes detected and rejected, plus "allows the delivered tree" |
| `python3 scripts/check_tui_editor_key_authority.py` | pass — "**2** guard-block chord(s), **72** Normal-mode chord(s), **8** Insert-mode chord(s), no shadowed arm" |
| `python3 scripts/check_tui_editor_key_authority.py --self-test` | pass — "10 case(s)" |
| `python3 scripts/check_test_assertion_quality.py` | pass — "all checks pass" |
| `python3 scripts/check_test_duplication.py` | pass — "all checks pass" |
| `cargo fmt --all -- --check` | exit 0, clean |
| `cargo check --workspace --all-targets --locked` | exit 0 (one pre-existing warning: an unused `Seek` import in `src/bin/codegg-sandbox-helper.rs`, a file this milestone does not touch) |
| `bash scripts/verify.sh quick` | **exit 0 — 73 of 73 steps green**, "Quick verification passed" |

**The gate's one previously-red step is now green.**
`check_test_target_references` compares the `architecture/overview.md`
integration-test-file count row against the tree. That row was stale —
it read 326 while the tree held 327 root integration test modules —
because this line's new members had been added without re-measuring it.
**The parent has since corrected the row to 327**, and
`check_test_target_references` now passes. A red gate on a
**documentation count row** is a **documentation obligation, not a
product defect**: nothing about the product's behaviour was ever in
question. §9 names the row.

### The full `--lib` run

The whole-lib run is the surface on which the environmental failures
appear. Its five failures are recorded here in full, with their
attribution, so that a reader is not left to discover them.

| Failure | Attribution |
|---|---|
| `core::daemon_refresh::tests::session_projection_rebuilds_owner_scoped_live_jobs` — `register workspace: InvalidRootPath("/tmp")` | **Environmental and pre-existing.** This host's `/tmp` root path is not usable by the daemon-refresh fixture. Nothing in this milestone touches the daemon. |
| `core::daemon_refresh::tests::session_projection_snapshot_includes_in_process_live_child_instances` — `register workspace: InvalidRootPath("/tmp")` | **Environmental and pre-existing.** Same cause. |
| `managed_process::tests::offline_required_scheduler_carried_intent_refuses_disabled_sandbox_before_spawn` — `OfflineRequired is unavailable on this platform` | **Environmental and pre-existing.** The macOS host has no offline-required sandbox backend. Nothing in this milestone spawns a process. |
| `tui::commands::interactive_terminal::tests::a_hidden_terminal_surface_does_not_arm_the_loop` — "the fixture must start with a mounted surface" | **Load-sensitive, not this milestone's.** It **passes in isolation** (`cargo test --locked -p codegg --lib tui::commands::interactive_terminal::tests::a_hidden_terminal_surface_does_not_arm_the_loop -- --exact` → 1 passed, 0 failed). `src/tui/commands/interactive_terminal.rs` has **no diff lines** against the baseline; it is a `multi_thread` tokio test that spawns a shell, and it failed only under the load of a full `--lib` run on this host. It is the same class as the `file_tree_coordinator` flake the sibling records already name. |
| `tui::commands::interactive_terminal::tests::panel_rendered_buffer_shows_live_pty_output` — "the panel adopted the new terminal" | **Load-sensitive, not this milestone's.** Same cause; it **passes in isolation** (1 passed, 0 failed) and its file has no diff lines. |

Two further notes on the same run:

- `tui::file_tree_coordinator::tests::barrier_held_worker_plus_100_rebuilds_keeps_one_worker_one_pending`,
  which the sibling records name as a load-sensitive flake, **passed** in
  this run. The set of load-sensitive failures is not stable across
  runs; that is the point of the attribution.
- The two documentation-count failures the sibling records name —
  `tui::command::tests::command_docs_builtin_slash_action_variant_count_matches_source`
  and `tui::command::tests::command_docs_count_matches_registry` — **no
  longer occur**. `architecture/command.md` now states **161**
  `BuiltinSlashAction` variants and **169** built-in commands, which is
  what the source holds. That row was one of this milestone's own
  counts, and it is now correct.

**Skipped scope.** `scripts/verify.sh full` (clippy `-D warnings` over
the whole workspace, the workspace test suite, and
`cargo test -p codegg --features server,plugins,lsp-test-support`) was
**not** run, and no claim is made about it. A hosted CI run was **not**
executed against this code; the evidence above is local. The change is
confined to the TUI's presentation and interaction path, and the commands
above cover the modules it touches.

## 5. Invariant review

| Invariant | Evidence | Result |
|---|---|---|
| **I1 — one producer for rows and for the row count.** The part boundaries are recorded by that producer, so the row count and the drawn rows continue to agree. | The boundaries are recorded inside the existing producer: `build_message_rows` returns `(Vec<Line>, Vec<PartSpan>)` and `push_message` takes the spans (`src/tui/components/messages.rs:1390`); no second producer exists. `the_scrollbar_follows_the_row_count_after_an_expand_all` (`tests/tui_render/tui_main_buffer_row_precise_disclosure.inc:265`) asserts the drawn rows equal the counted rows **after** an expand-all, and that the scroll range comes from the same count. | pass |
| **I2 — the TUI renders what the daemon sent.** | The milestone adds **bookkeeping** to the producer and **resolution** on top of it. No `MsgPart` is synthesized, dropped, filtered or reordered; `set_all_disclosures_expanded` only flips the `expanded` / `collapsed` flags on parts the daemon sent. The model-facing path is byte-identical: `git diff f2639b5d2 -- src/search_backend/` is **zero lines**. | pass |
| **I4 — nothing is silently unreachable.** The milestone's whole purpose. | Before the milestone, the tool call in a message carrying both a reasoning block and a tool call was **unreachable by clicking**: the reasoning arm won every time. Now a tool-call row resolves to the tool call (`clicking_a_tool_call_row_expands_the_tool_call_and_not_the_reasoning`) and a reasoning row to the reasoning block (`clicking_a_reasoning_row_expands_the_reasoning_block`). | pass |
| **I5 — a wrap never breaks an escape sequence.** Unchanged. | This milestone changes **no** wrapping. The tool-output body still goes through the plain `wrap_to_strings` engine, whose pre-existing lack of escape-atomicity is recorded in §10 and was recorded by M001 and M003 before it. | pass |
| **I6 — model prose is not collapsed.** Expand-all expands **disclosure state**; it does not change model prose, which is not collapsed in the first place. | `expanding_every_disclosure_does_not_touch_the_prose` (`src/tui/components/messages.rs:6066`); the implementation folds only `MsgPart::ToolCall` and `MsgPart::Reasoning` (`src/tui/components/messages.rs:1197`), and the `MsgPart::Text` arm is not a disclosure. | pass |
| **I7 — the live `Ctrl+O` binding is unchanged.** The workspace dashboard keeps `Ctrl+O` in both maps. | `the_workspace_dashboard_keeps_ctrl_o_in_both_maps` (`src/tui/input.rs:3635`) asserts `(CONTROL, Char('o'))` → `OpenWorkspaceDashboard` in the default map **and** the vim map. The new chord is `(CONTROL\|ALT, Char('o'))`, a different key. | pass |

## 6. Failure and recovery review

- **Failure.** The row→part resolution is total: a row inside the buffer
  resolves to a part or to nothing, a row past the end of the buffer
  resolves to nothing, and the handler's `None` arm is an explicit
  no-op. The expand-all is a bounded fold over the transcript's parts.
  No input makes either path panic.
- **Duplicate delivery and idempotency.** Not applicable. The
  disclosure state is frontend-local and is not delivered by anything.
  The chord's **repeat** is the gesture's contract, not an idempotency
  question: the second press collapses what the first expanded, which
  is asserted by `the_expand_all_gesture_opens_everything_and_the_next_press_closes_it`.
- **Cancellation races.** Not applicable. Nothing asynchronous is added.
  The click handler and the chord are synchronous mutations of
  frontend-local state, and there is no cancellable operation.
- **Daemon or node restart.** Not applicable. Nothing is persisted. The
  disclosure state is a field of the `MsgPart` the daemon sends, and
  the collapsed-by-default construction (`collapsed: true`) is the
  state a restarted session comes back to.
- **Partial persistence failure.** Not applicable. No write path is
  added and nothing is written to disk.
- **Stale generation or lease.** Not applicable in the durable sense —
  there is no generation counter and no lease. The frontend analogue is
  the **layout cache**, and it is handled: `toggle_tool_call_part`,
  `toggle_reasoning_part` and `set_all_disclosures_expanded` each call
  `invalidate_rows_from`, the aggregate from row 0. A cached row count
  therefore cannot survive a disclosure change, which is what the
  expand-all's row-count assertion checks.
- **Contention and resource release.** Not applicable. The producer is
  frontend-local and single-threaded within a render. The resolution is
  a binary search over the part offsets — O(log n) in the number of
  expandable parts. There is no shared resource and nothing to release.
- **Malformed or unauthorized input.** A viewport row that is past the
  end of the buffer, or that owns no part, resolves to `None` and
  toggles nothing. The expand-all acts only on parts the daemon sent,
  and only flips disclosure flags on them; it cannot introduce
  content. The click handler still resolves the message for
  **selection** (`select_at_viewport_line`), so a click on an
  unselectable row clears the selection rather than toggling a part.
- **Bounded event and artifact behavior.** No event, artifact or file is
  produced. The row count after an expand-all is bounded by the
  transcript's content, and the scrollbar follows it.

## 7. Migration and compatibility review

- **Schema migration.** None. Nothing is persisted, so there is no
  migration and no storage-layout version to move.
- **Protocol.** None. No daemon message, session state, or wire format
  changes. The model-facing path is byte-identical (I2).
- **Configuration.** None. The gesture is not user-configurable through
  a config key. It **is** rebindable through the existing keybind
  editor, because the action is an `ActionKey` — that is the existing
  mechanism, reused, not a new configuration surface.
- **Backward compatibility.** Two behaviour changes and one
  non-change, all recorded:
  - **The click's meaning changes** for a message carrying both a
    reasoning block and a tool call: a click now toggles the **part**
    under the cursor rather than always the reasoning. This is the
    defect fix, and it is a user-visible behaviour change.
  - **The built-in command registry grows by one** — 168 → 169 — with
    `/expand-all` and its two aliases. `architecture/command.md` pins
    that count and states 169 and 161 `BuiltinSlashAction` variants.
  - `Ctrl+O` is **unchanged**; the new chord is additive.
- **Rollback limitations.** The change is self-contained in
  `src/tui/components/messages/layout.rs`,
  `src/tui/components/messages.rs`, `src/tui/app/mod.rs`,
  `src/tui/input.rs`, `src/tui/command.rs` and the two guard scripts.
  Reverting those restores the previous behaviour exactly — including
  the old message-level click behaviour. There is no persisted state to
  migrate back and no compatibility shim to unwind.
- **Legacy paths.** The message-level row→message resolution
  (`find_message_at_line`, `select_at_viewport_line`) is **retained**,
  not deprecated: message selection still uses it. The plain
  `wrap_to_strings` engine is retained for the tool-output path, with
  its pre-existing lack of escape-atomicity recorded in §10.

## 8. Security review

- **Authorization.** Unchanged. This milestone adds no key handling, no
  tool path, no daemon interaction, and no authorization decision. The
  gesture expands content the user is already entitled to see — the
  disclosures are the user's own transcript.
- **Secrets.** None handled. No credential, token, or path is read,
  resolved or logged.
- **Privilege boundaries.** Unchanged.
- **Path validation.** Not applicable — the milestone parses no paths
  and resolves none.
- **The trust frame.** The frame is a **prompt-injection defence** and
  it belongs to the model path. This milestone does not weaken it: it
  does not read the frame, does not parse it, and does not touch
  `src/search_backend/framing.rs`, which has **zero diff lines**
  (`git diff f2639b5d2 -- src/search_backend/` is zero lines). The
  model still receives the frame byte for byte.
- **The interaction contract.** The chord contract is `{CONTROL, ALT}`
  only, and it is **unchanged and not weakened** by this milestone —
  the milestone added a chord that obeys it. The owner's chosen
  `Ctrl+Shift+O` was **not** deliverable in this TUI, because the app
  never pushes keyboard-enhancement flags (`src/tui/terminal.rs`
  enables raw mode, bracketed paste and mouse capture only), so in
  legacy xterm encoding that keystroke arrives indistinguishable from
  `Ctrl+O` and would have opened the workspace dashboard. The
  repository already forbids a `CONTROL|SHIFT` chord in either binding
  map — `tests/tui_render/modifier_chords.inc::no_binding_map_contains_a_control_shift_entry`
  — and states the contract in `architecture/tui.md`
  ("Chord contract"), `docs/tui.md` and `src/tui/input.rs:966`. The delivered
  `Ctrl+Alt+O` arrives as ESC plus one control byte, which crossterm
  reports as `CONTROL|ALT`: unambiguous against both `Ctrl+O` and bare
  `Alt+O`. **`Ctrl+O` was not re-spelled.** The substitution is a
  deviation from the owner's stated choice and is a named finding in
  §10, with the owner's ratification outstanding.
- **Denial of service.** The expand-all expands every disclosure in one
  press, so the row count grows with the transcript's content — that is
  the gesture's purpose, and the scrollbar follows the count (I1). The
  cost is bounded by the content already in the buffer: the fold is
  O(parts) and the re-layout is the same work a scroll to the end of
  the transcript already causes. Nothing is fetched, spawned or written.
- **Redaction.** Unchanged. The milestone renders and toggles bytes it
  is given; it adds no redaction and removes none.
- **Audit.** No new audit event. Toggling a disclosure is a rendering
  and interaction action, not an auditable one.

## 9. Documentation and operations

- **Updated architecture documents.**
  - `architecture/tui.md` — "A row resolves to its part, not to its
    message" and "The expand-all gesture": the part boundaries and the
    resolution path, the three click outcomes, the chord **and why it
    is `Ctrl+Alt` and not `Ctrl+Shift`**, the palette command, the
    toast, and the guard repairs.
  - `architecture/command.md` — the `/expand-all` row with its aliases,
    and the corrected built-in counts (169 commands, 161
    `BuiltinSlashAction` variants).
  - `architecture/overview.md` — the integration-test-file count row,
    **327**, which this milestone's new member moved.
- **Updated user-facing documentation.**
  - `docs/tui.md` — "Opening everything at once": the `Ctrl+Alt+O`
    chord, the `/expand-all` aliases, and the two toast strings. The
    chord-contract sentence ("Keybindings with two modifiers use
    `Ctrl+Alt`, never `Ctrl+Shift`") was already there and is
    unchanged.
- **Updated module guide.** `.opencode/skills/tui/SKILL.md` — the
  expand-all gesture, the substitution and its reason, and the composer
  guard's five-chord model.
- **Static guards.** **None added**, by design: the plan's work package
  D asked for the existing guards to be **repaired** and their
  `--self-test`s to still reject their defect classes, and both were.
  - `scripts/check_tui_composer_key_authority.py` — the chord-bound
    action set widened to five, the rule text widened from "re-spelled"
    to "chord-bound", and the **brace matcher repaired** to be
    comment- and literal-aware. Its `--self-test` passes and still
    detects every violation class, including "detects a chord-only
    action listed in is_normal_only".
  - `scripts/check_tui_editor_key_authority.py` — two stale prose
    counts corrected (66 → 72 Normal-mode chords; "three" → two
    guard-block chords). Its `--self-test` passes.
  - The existing test-quality guards `check_test_assertion_quality.py`
    and `check_test_duplication.py` cover the new tests and pass.
- **Operator diagnostics.** None. A disclosure gesture has no
  operational surface: nothing to configure, nothing to diagnose, no
  log line to read.
- **The documentation obligation from the gate's one red step is now
  discharged.** `architecture/overview.md`'s integration-test-file count
  row was stale at 326 against a tree holding 327 root integration test
  modules; **the parent has corrected it to 327**, and
  `check_test_target_references` passes. It was a **documentation count
  row**, not a product behaviour, and §4 records it as such.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | **The hosted `CI / verify` lane has not run against the commit that carries this code.** The implementation and this record land together in one commit on `chore/record-landing-lane-outcomes` (baseline `f2639b5d2`). | The milestone's evidence is local only. A local `verify.sh quick` run is a property of this machine and this tree; the hosted lane is a strict **subset** of `quick`, so neither the local run nor a future hosted run proves the other. | **Required before this record can be upgraded to `closed`:** a green hosted `CI / verify` run against this commit on the hosted runner. |
| medium | **The owner's chosen expand-all chord was not deliverable, and shipped as `Ctrl+Alt+O`.** The owner chose `Ctrl+Shift+O`; the milestone shipped `Ctrl+Alt+O`. The app never pushes keyboard-enhancement flags (`src/tui/terminal.rs` enables raw mode, bracketed paste and mouse capture only), so in legacy xterm encoding a `Ctrl+Shift+O` keystroke arrives **indistinguishable from `Ctrl+O`** and would have opened the workspace dashboard instead; the repository already forbids a `CONTROL\|SHIFT` chord in either binding map (`tests/tui_render/modifier_chords.inc::no_binding_map_contains_a_control_shift_entry`) and states the contract in `architecture/tui.md` ("Chord contract"), `docs/tui.md` and `src/tui/input.rs:966`. | The **capability is complete and discoverable** — `Ctrl+Alt+O` in both maps, plus `/expand-all` (aliases `expand-disclosures`, `collapse-all`) and a toast that names the outcome — but the **gesture the owner named is not the one that shipped**, and the milestone made that substitution on its own reading of the platform. `Ctrl+O` was **not** re-spelled, so no existing binding was lost. | **The owner must take one of two actions.** (1) **Ratify `Ctrl+Alt+O`** as the expand-all chord — the deliverable that is in the tree — or (2) **ask for the terminal layer to push keyboard-enhancement flags**, so `Ctrl+Shift+O` becomes deliverable, which is a larger change to `src/tui/terminal.rs` and **outside this milestone's scope**. Until one of the two happens, the chord is a **deviation from the owner's stated choice**, not a settled decision. |
| medium | **The plain `wrap_to_strings` engine is still not escape-atomic.** A tool output that contains an escape sequence can still be split by the plain word wrap. | A tool output carrying a terminal control sequence can have that sequence split across a row boundary, which corrupts the sequence for the terminal. | **Pre-existing** and **outside this milestone's scope**: the baseline had the same property, the tool-output path was explicitly out of scope for M001, M003 and M004 alike, and this milestone changes no wrapping. Recorded for a future milestone that moves the tool-output path onto the styled primitive. |
| medium | **The transcript's search highlight is broken and the search scroll mixes a visual row offset with a logical line count.** `highlight_match_in_rendered` compares a byte offset against display columns, and `scroll_to_current_search_match` adds a visual row offset to a logical line count. | A search match in the main buffer can be missed or highlighted on the wrong column, and the scroll can land on the wrong line. The defect is **pre-existing** and unchanged by this milestone. | Recorded, **not fixed**: the roadmap places this in risk **R4**, and M001's scope is the primitive. The three `_known_defect` tests make the defect observable and name it. Tracked by the roadmap; a future milestone owns the fix. |
| low | **The `plans/registry.md` M004 row's narrative cell still names `Ctrl+Shift+O` as the bound chord.** The registry's *line* narrative paragraph (the one that covers both text-rendering lines) records the substitution and the delivered `Ctrl+Alt+O` correctly, and the main-buffer roadmap's M004 note does too — but the per-milestone row cell still carries the plan's original wording. | A reader of the registry row alone would believe the milestone bound a `Ctrl+Shift+O` chord. Nothing about the product's behaviour is affected; the code binds `Ctrl+Alt+O`. | A **documentation correction** on the branch: the row's narrative cell should name `Ctrl+Alt+O` and the reason, matching the roadmap's note and the registry's own line narrative. Owned by the line's owner, not by this milestone. |

## 11. Roadmap disposition

**Milestone conditionally closed with named operational evidence outstanding.**

The production implementation is complete and locally verified: the
part boundaries, the row→part resolution, the click handler, the
expand-all gesture, the palette twin and the guard repairs are all in
the tree; the tests are in the tree; the local gate is **73 of 73 steps
green**. The **outstanding evidence is named**:

1. a **closing commit** that carries the working tree (there is no SHA
   today), and
2. a **green hosted `CI / verify` run against that commit on the hosted
   runner**.

The **risk** is stated in §1: a local `scripts/verify.sh quick` run is a
property of this machine and this tree, not of the hosted runner, and
the hosted lane is a strict **subset** of `quick`, so a green hosted run
does not by itself prove everything the local gate proves.

A **second, named condition** rides on it: the owner's chosen chord
`Ctrl+Shift+O` was not deliverable, the milestone shipped `Ctrl+Alt+O`,
and the owner's ratification of that substitution is outstanding
(§10). The capability itself is complete, so this does not make the
milestone blocked — but it is a decision the owner still owns.

M004's **M003 (hard)** dependency is **discharged**: M003's collapsed
one-line layout and per-part disclosure state are in the tree and are
what M004 drives. The search defects stay recorded under the roadmap's
risk R4 and are not absorbed into this milestone.

## 12. Registry updates

The parent owns `plans/registry.md` and the roadmaps; the changes below
are **already applied**, and this section states what they now say.

- `plans/registry.md` — the "TUI main buffer text rendering" line's
  **M004 row** (the row titled *"TUI main buffer text rendering | M004
  row-precise disclosure and expand-all"*) now reads:
  - **Status**: `**conditionally closed**`.
  - **Implementation plan**: `plans/implementation/tui-main-buffer-text-rendering/004-row-precise-disclosure-and-expand-all.md`.
  - **Closure record**: `plans/closure/tui-main-buffer-text-rendering/004-status.md`.
  - **Narrative**: the baseline `f2639b5d2`; the baseline defect
    (the handler resolving the **message** and asking "has reasoning"
    before "has tool output"); the fix (the row resolves to the **part**
    that owns it, and the boundaries must be recorded **inside** the
    producer or the row count and the drawn rows drift apart); the chord
    and its additivity; the palette mirror; the guard repair; and the
    deliberate deferral of keyboard *selection* of a collapsed call.
  - One **residual** in that row: its narrative cell still names
    `Ctrl+Shift+O` as the bound chord. The registry's **line** narrative
    paragraph — the one covering both text-rendering lines — records the
    substitution and the delivered `Ctrl+Alt+O` with its reason, so the
    registry as a whole is not wrong; only the row cell is stale. It is
    named in §10 as a low-severity documentation correction.
- `plans/registry.md` — the **line narrative paragraph** for "TUI text
  rendering" records that both roadmaps are **conditionally closed**, that
  the implementation lands in a commit and that the hosted
  `CI / verify` lane has not run against that commit, that the expand-all
  chord was chosen as `Ctrl+Shift+O` and is **not deliverable** and
  therefore shipped as **`Ctrl+Alt+O`** beside `/expand-all` with
  `Ctrl+O` **not** re-spelled, that the two findings are **recorded, not
  fixed**, and that **no new static guard** was added by this work.
- `plans/subsystems/tui-main-buffer-text-rendering-roadmap.md` —
  §13 "Milestone status": the **M004 row** reads
  `conditionally closed`, with the implementation plan path and this
  closure record's path filled in and `M003 (hard)` as the blocker.
  The roadmap's **Outcome (2026-10-23)** paragraph records that M001–M004
  are implemented and locally verified, that the line is conditionally
  closed, and that the closing commit and the hosted run are the named
  outstanding evidence. Its **M004 note** records the chord substitution
  with the legacy-encoding reason and names the two recorded-not-fixed
  findings.
- `plans/subsystems/tui-composer-text-layout-roadmap.md` — the **peer
  line's** table reads `conditionally closed` for its own milestones.
  That line is outside this milestone's scope and is closed by its own
  record; it is named here only because the registry paragraph above
  covers both lines together.
