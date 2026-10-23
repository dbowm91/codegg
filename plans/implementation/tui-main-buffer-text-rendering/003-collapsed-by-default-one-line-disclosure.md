# TUI Main Buffer Text Rendering Milestone M003 — Collapsed by Default: One Line per Tool Call and per Reasoning Block

Status: implemented — closure record at `plans/closure/tui-main-buffer-text-rendering/003-status.md`

Repository baseline: `f2639b5d2` (`origin/main`)

Source roadmap: `plans/subsystems/tui-main-buffer-text-rendering-roadmap.md#milestone-m003--collapsed-by-default-one-line-per-tool-call-and-per-reasoning-block`

Long-term requirements:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`

Applicable ADRs:

- None.

Primary class: capability (roadmap C2, C3) with a polish obligation (roadmap P1)

## 1. Objective

Make a collapsed tool call **exactly one line** and a collapsed reasoning block
**exactly one line**, so a screen of tool calls can be skimmed.

The disclosure state machinery already exists. This milestone changes defaults,
tightens the collapsed layout, and removes the preview rows from the collapsed
state.

## 2. Why this milestone is ready

- **M001 (hard)** is closed: the wrap primitive exists, so the one-line
  header's ellipsis is measured against the same width the wrapped body uses
  and the two cannot disagree.
- **M002 (interface)** is closed: the header's summary is read from the
  extracted payload, so a framed tool output's summary is the result rather
  than the banner.
- **The disclosure state already exists.** `expanded` and `collapsed` are
  per-part fields with toggles; this milestone changes their defaults and the
  rendering, not the model.

## 3. Current implementation evidence

Verified at baseline `f2639b5d2`.

### 3.1 The collapsed tool call is up to ten rows

`TOOL_OUTPUT_PREVIEW_LINES` is 8. A collapsed tool call renders the header,
plus up to 8 preview rows of the tool's output, plus a
"(+N more lines, click or toggle to expand)" hint row. The header itself is
word-wrapped, so it can be several rows on its own.

### 3.2 The header already carries the pieces the one line needs

The header is built from the tool's name, a target extracted from the input
JSON, and a summary of duration, output lines and exit code. The target
extraction already knows the tool's interesting field per tool family —
the path for a file tool, the command for a shell tool, the prompt for a
delegation — and falls back to the first string value in the input object.

The target currently has a 400-column ceiling, which is a **display-width**
bound rather than a row bound. Under a one-line header the bound becomes the
row's remaining width.

### 3.3 The reasoning block prints its whole body by default

`MsgPart::Reasoning` carries `collapsed: bool`, and every construction site
sets it to `false`. The collapsed rendering already exists and is already
exactly what the owner asked for: a single row reading `Thinking` or
`Thought for <duration>`.

A global `show_thinking` switch and a `thinking` palette command exist and
work.

### 3.4 The click target is the message

The click handler checks "does this message have reasoning?" **before** "does
it have tool output?", and "has reasoning" is true when the message has any
reasoning part, expanded or not. In a message with both, the first click
expands the reasoning and the second collapses it again — the tool call is
never reachable by clicking.

This milestone does not fix it (M004 does). It must, however, not make it
worse: the collapsed layout must not remove the click's ability to reach
something.

## 4. Invariants that must not regress

- **I1 — one producer for rows and for the row count.**
- **I2 — the TUI renders what the daemon sent.**
- **I3 — the trust frame stays in the model path.** Unchanged; M002 owns it.
- **I4 — nothing is silently unreachable.** The collapsed view shows less;
  the content must remain reachable by expansion. This is the invariant the
  collapsed layout is judged against.
- **I5 — a wrap never breaks an escape sequence.**
- **I6 — model prose is not collapsed.** The owner's decision D10: only tool
  calls and reasoning blocks collapse. An assistant answer may occupy as many
  rows as it needs.

## 5. Scope

### In scope

- A collapsed tool call renders as **exactly one row**.
- A collapsed reasoning block renders as **exactly one row**.
- The one-line header's shape: the tool's icon, its name, its target
  (ellipsized to the remaining width), and a summary of duration and hidden
  output size; the hidden-output counter is folded into that same row.
- The hidden-output counter is **removed** from the header when the call is
  expanded.
- Reasoning blocks default to collapsed.
- The preview rows and the "(+N more lines)" hint row are absent from the
  collapsed state.

### Explicitly out of scope

- Fixing the click target (M004).
- The expand-all gesture and the `Ctrl+Alt+O` chord (M004).
- Collapsing model prose (D10 — never).
- The unwrapped paths' content (roadmap D9: the wrap and the collapse are the
  remedy; the remaining unwrapped metadata paths are out of this milestone's
  scope unless the collapsed layout removes their clipping).
- The `(multiline)` marker row in the composer. That is the other roadmap's
  decision (D6 there), kept for now.

## 6. Required production changes

### Core/domain

None.

### Storage and migrations

None. The disclosure state is frontend-local and already is.

### Protocol and DTOs

None.

### Runtime and concurrency

None.

### Frontend or operator surface

**The collapsed tool call is one row.** The preview rows and the
"(+N more lines)" hint row do not render while the call is collapsed. The
hidden-output count is folded into the header row instead, so the affordance
survives on one line.

**The header is ellipsized, not wrapped.** The header is currently word-wrapped
and can occupy several rows. It becomes a single row whose content is
ellipsized to the row's width. The ellipsis is applied to the **target**, not
to the whole header, so the tool's name, its icon and the expand affordance
survive.

The target's current 400-column ceiling is replaced by "the row's remaining
width", computed after the fixed parts (icon, name, summary, counter) are
laid out. At a width narrower than the tool's name, the header still
ellipsizes rather than wrapping, and the affordance survives where it can.

**The expanded header drops the counter.** With the content visible, the count
of hidden lines is noise (owner decision D5).

**Reasoning defaults to collapsed.** Every construction site of a reasoning
part sets `collapsed: true`. The global `show_thinking` switch and the
`thinking` palette command keep working: `show_thinking = false` still hides
reasoning entirely, and `show_thinking = true` shows the collapsed one-line
blocks, which the user can then expand individually.

### Security and authorization

None. The collapsed view shows less of the same untrusted content; the content
is still rendered as text and is still reachable.

### Documentation and static guards

- `architecture/tui.md` — the collapsed layout's contract and the header's
  shape.
- `docs/tui.md` — what is collapsed by default and how to see more.
- `.opencode/skills/tui/SKILL.md` — if it describes the collapsed layout.
- **No new static guard.**

## 7. Ordered work packages

### Work package A — The header becomes one row

Intent: the header is a single ellipsized row.

Required changes: replace the header's wrapping with ellipsis-to-width; apply
the ellipsis to the target; compute the remaining width after the fixed parts;
keep the icon, the name and the affordance.

Acceptance evidence: a tool call with a long target renders as one row at
several widths, including a width narrower than the tool's name; the target
carries the ellipsis; the name does not.

### Work package B — The collapsed state drops the preview

Intent: the collapsed call is the header and nothing else.

Required changes: the preview rows and the "(+N more lines)" hint row do not
render while collapsed; the hidden-output count moves into the header row;
the expanded state drops the count from the header.

Acceptance evidence: a collapsed tool call contributes exactly one row to the
message's row count; an expanded one contributes its full rows; the count
appears in the collapsed header and not in the expanded header.

### Work package C — Reasoning defaults to collapsed

Intent: a reasoning block is one line unless the user asks otherwise.

Required changes: the construction sites default `collapsed: true`; the
global switch and the palette command keep their meaning.

Acceptance evidence: a message with a reasoning block contributes one row for
it by default; expanding it contributes its body; the `thinking` palette
command still toggles the global switch.

### Work package D — The documents

Intent: the documented default matches the shipped default.

Required changes: update `architecture/tui.md` and `docs/tui.md`; verify every
count they state against its listed source.

Acceptance evidence: the documents state that tool calls and reasoning blocks
are collapsed by default, and that model prose is not.

## 8. Failure, cancellation, restart, and contention semantics

Not applicable. Frontend-local; no durable state; no concurrent writer.

## 9. Compatibility and migration

- **Rendering only.** No stored state, no protocol, no migration.
- **A default change is a behaviour change.** Users who relied on a tool
  output being visible without expanding it will now expand it. The change is
  recorded in the closure record and in the user-facing documentation.
- **The disclosure state was already persisted in the frontend session**, so a
  user's existing expansion choices survive the default change.

## 10. Required tests

### Focused unit tests

- The header renders as one row at several widths: a wide row, a row
  narrower than the target, and a row narrower than the tool's name.
- The ellipsis lands on the target, not on the name or the affordance.
- The hidden-output count appears in the collapsed header and is absent from
  the expanded header.
- A collapsed tool call contributes exactly one row; an expanded one
  contributes more.
- A reasoning part defaults to collapsed and renders one row.

### Integration tests

- A rendered transcript containing several tool calls and a reasoning block
  has a row count equal to one row per collapsed call, plus the prose rows.
- The scrollbar's `total_lines` equals the number of rows drawn (I1).
- Clicking a collapsed tool call still reaches an expanded state (the
  milestone must not make the content unreachable — I4, even though the click
  *target* is M004's to fix precisely).

### Restart and recovery tests

None.

### Contention and cancellation tests

None.

### Security and negative tests

- A tool call whose output is untrusted content is still rendered as text and
  never as markup or a command.
- A tool call with an escape sequence in its output is not split (I5).

### Migration and compatibility tests

None.

### Non-vacuity

Every new test must fail against the baseline tree: against the baseline, a
collapsed tool call contributes **up to ten** rows, not one.

## 11. Required verification commands

```bash
# the collapsed layout and the header, narrowly
cargo test --locked -p codegg --lib tui::components::messages

# the rendered transcript
cargo test --locked -p codegg --test tui_render

# static guards that cover the new tests
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py

# formatting and the workspace check
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked

# the routine gate
bash scripts/verify.sh quick
```

## 12. Documentation updates

- `architecture/tui.md` — the collapsed layout, the header's shape, the
  ellipsis rule, and the explicit statement that model prose is not
  collapsed.
- `docs/tui.md` — the collapsed-by-default behaviour and how to expand.
- `.opencode/skills/tui/SKILL.md` — if it describes the layout.
- `plans/registry.md` — this milestone's row, when it closes.

## 13. Acceptance criteria

- A collapsed tool call occupies exactly one row, at every width.
- A collapsed reasoning block occupies exactly one row.
- The one-line header shows the tool's icon, its name, an ellipsized target
  and a summary; the hidden-output count is in that row.
- An expanded tool call shows its content in full and its header without the
  hidden-output count.
- Model prose is **not** collapsed: an assistant answer occupies the rows it
  needs.
- The content of a collapsed tool call is reachable by clicking it.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- the one-line header cannot be made to fit at a narrow width without
  dropping the expand affordance;
- making the header one row would require changing the tool-call data model;
- the collapsed layout would remove the last route to a tool call's content
  before M004 lands;
- scope would expand into the click targets or the expand-all gesture
  (M004).

## 15. Closure evidence required

- The header's and the row-count's unit tests.
- A rendered transcript showing several collapsed tool calls, each on one
  row, at two widths.
- The scrollbar's row-count assertion (I1).
- The click-reachability assertion (I4).
- The disposition of the existing transcript tests that assert the old
  multi-row collapsed layout.
- `scripts/verify.sh quick` green at the closing commit.

## 16. Handoff notes

- The header has fixed parts and a variable part. The ellipsis belongs on the
  variable part; a whole-header ellipsis will eat the affordance.
- The hidden-output count must move, not disappear: it is how the user knows
  there is something to expand.
- This milestone changes a default. Users will notice. The closure record
  says so plainly rather than describing it as a neutral refactor.


## Correction (2026-10-23) — the expand-all chord

**The chord in this plan is not deliverable, and the implementation shipped a
different one.** The plan reasons that no `Ctrl+Shift` combination is bound in
either map, so `Ctrl+Shift+O` is free. The premise is true and the conclusion
is not: the app **never pushes keyboard-enhancement flags**
(`src/tui/terminal.rs` enables raw mode, bracketed paste and mouse capture
only), so in legacy xterm encoding a `Ctrl+Shift+O` keystroke arrives
**indistinguishable from `Ctrl+O`** and would have fired the workspace
dashboard instead of expanding anything. The repo already forbids a
`CONTROL|SHIFT` chord in either binding map —
`tests/tui_render/modifier_chords.inc::no_binding_map_contains_a_control_shift_entry`
is a structural assertion over the parsed maps — and states the contract in
`architecture/tui.md` ("Chord contract. Multi-modifier chords are
`{CONTROL, ALT}` only"), in `docs/tui.md` and in `src/tui/input.rs`.

The implementation therefore bound the gesture to **`Ctrl+Alt+O`** in both
binding maps, with the palette command **`/expand-all`** beside it, and left
`Ctrl+O` on the workspace dashboard. The owner's choice of `Ctrl+Shift+O` was
**not deliverable**; delivering it would need the terminal layer to push
keyboard-enhancement flags, which is a larger change to
`src/tui/terminal.rs` and outside this milestone's scope. The substitution is
recorded as a **named finding** in
`plans/closure/tui-main-buffer-text-rendering/004-status.md` and is the
owner's to ratify.
