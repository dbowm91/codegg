# TUI Main Buffer Text Rendering Milestone M004 — Row-Precise Disclosure and Expand-All

Status: implemented — closure record at `plans/closure/tui-main-buffer-text-rendering/004-status.md`

Repository baseline: `f2639b5d2` (`origin/main`)

Source roadmap: `plans/subsystems/tui-main-buffer-text-rendering-roadmap.md#milestone-m004--row-precise-disclosure-and-expand-all`

Long-term requirements:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`

Applicable ADRs:

- None. The gesture is a frontend interaction, not an ownership, identity,
  protocol, authorization or durability decision.

Primary class: capability (roadmap C5, C6)

## 1. Objective

Make clicking expand **the thing on the row that was clicked**, and add one
gesture that expands **everything** at once.

Today the click target is the message, so in a message carrying both a
reasoning block and a tool call, the tool call cannot be reached by clicking
at all. And there is no gesture that opens the whole transcript for reading.

## 2. Why this milestone is ready

- **M003 (hard)** is closed: the collapsed one-line layout and the per-part
  disclosure state it settles are what this milestone drives.
- **M001 and M002** are closed, so the row producer already produces the rows
  this milestone resolves against, and the summary a header shows is the
  extracted payload rather than the banner.
- **A free chord exists.** No `Ctrl+Shift` combination is bound anywhere in
  the binding maps today, so a two-modifier chord is available without
  re-spelling any live binding. `Ctrl+O` is the workspace dashboard and
  stays that way. **See the dated correction at the end of this document:**
  the chord is `Ctrl+Alt+O`, not `Ctrl+Shift+O`, because `Ctrl+Shift` is
  not deliverable in this terminal.

## 3. Current implementation evidence

Verified at baseline `f2639b5d2`.

### 3.1 The click target is the message, and the reasoning arm wins

The click handler asks "does this message have reasoning?" first and "does it
have tool output?" second. The reasoning test is true when the message has
**any** reasoning part, expanded or not.

So in a message carrying both:

- click 1 → the reasoning expands
- click 2 → the reasoning **collapses again**
- the tool call is **never** reachable by clicking

The same handler reaches the reasoning even when the clicked row is a tool
call's header, because the message is the unit of resolution.

### 3.2 The row→message resolution already exists; the row→part resolution does not

The cache answers "which message owns this viewport row" through a binary
search over the cached per-message row ranges. It does **not** record where
one part's rows end and the next part's begin, so the clicked row cannot be
resolved to the part that owns it.

The cache is the one producer of rows and of the row count (I1), so this is
where the part boundaries belong.

### 3.3 There is no expand-all

Nothing in the TUI expands or collapses every disclosure at once. The
per-part toggles exist; there is no aggregate.

### 3.4 `Ctrl+O` is taken and `Ctrl+Shift` is free

`Ctrl+O` is bound to the workspace dashboard in both binding maps, and the
dashboard row in the registry records that this binding must stay live in
Insert mode.

No `Ctrl+Shift` combination is bound in either map. The owner's decision
was that `Ctrl+Shift+O` is the chord. **That chord is not deliverable** —
the app never pushes keyboard-enhancement flags, so a `Ctrl+Shift`
keystroke arrives indistinguishable from `Ctrl+O`. The chord shipped is
**`Ctrl+Alt+O`**; see the dated correction at the end of this document.

### 3.5 The command palette is the discoverable surface

The palette already carries a `thinking` command that toggles the global
switch, and a toast reports the result. The expand-all gesture should have
the same discoverability.

## 4. Invariants that must not regress

- **I1 — one producer for rows and for the row count.** The part boundaries
  are recorded by that producer, so the row count and the drawn rows continue
  to agree.
- **I2 — the TUI renders what the daemon sent.**
- **I4 — nothing is silently unreachable.** The milestone's whole purpose.
- **I5 — a wrap never breaks an escape sequence.** Unchanged.
- **I6 — model prose is not collapsed.** Expand-all expands **disclosure
  state**; it does not change model prose, which is not collapsed in the
  first place.
- **I7 — the live `Ctrl+O` binding is unchanged.** The workspace dashboard
  keeps `Ctrl+O` in both maps.

## 5. Scope

### In scope

- The click target resolves to the **part** that owns the clicked row.
- Clicking a tool-call header row expands **that** tool call.
- Clicking a reasoning header row expands **that** reasoning block.
- A new `Ctrl+Alt+O` chord that expands every hidden detail, and collapses
  every detail when pressed again.
- A palette command that mirrors the chord, with the same toast.
- The part boundaries recorded by the row producer.

### Explicitly out of scope

- The collapsed layout (M003) and the detail view (M002).
- Re-spelling `Ctrl+O`.
- Any new `Ctrl+Shift` chord other than this one.
- Per-tool **keyboard** selection of a collapsed call — that is, selecting
  which collapsed call the next key acts on. The click is the pointer path;
  the keyboard path is deferred.
- Changing the global `show_thinking` switch's meaning.

## 6. Required production changes

### Core/domain

None.

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

None.

### Frontend or operator surface

**Part boundaries in the row producer.** The cache records, per message, which
rows belong to which part — the tool call's header row, the reasoning
block's header row, the prose rows — so a viewport row resolves to a
`(message, part)` pair rather than to a message. This is an addition to the
existing producer's bookkeeping, not a new producer, so I1 holds by
construction.

**The click handler resolves the part.** Given the clicked row, the handler
resolves the owning part and toggles **that** part. A row that belongs to a
tool call toggles the tool call; a row that belongs to a reasoning block
toggles the reasoning block; a row that belongs to neither toggles nothing.

**The expand-all gesture.** A new action expands every tool call and every
reasoning block in the transcript, and a second press collapses every one.
It is bound to `Ctrl+Alt+O` in **both** binding maps, and it is added to
the command palette so it is discoverable without knowing the chord. It
reports its outcome in the same toast surface the `thinking` command uses.

**The key-authority guards.** The binding maps are policed by the existing
composer and editor key-authority guards. Adding a chord to both maps must
keep those guards green, and their recorded chord counts must be updated to
the new value rather than left stale — the guards treat a count as part of
the contract.

### Security and authorization

None. The gesture expands content the user is already entitled to see.

### Documentation and static guards

- `architecture/tui.md` — the disclosure model: the three affordances, the
  row→part resolution, and the expand-all gesture.
- `docs/tui.md` — the `Ctrl+Alt+O` chord and the click behaviour.
- `.opencode/skills/tui/SKILL.md` — the same, if it describes the gestures.
- **No new static guard.** The existing key-authority guards are **repaired**
  — their chord counts and any model of the binding maps are updated — and
  their `--self-test`s must still reject their defect classes.

## 7. Ordered work packages

### Work package A — The row producer records part boundaries

Intent: a viewport row resolves to a part.

Required changes: extend the layout cache's bookkeeping to record each part's
row range within its message, alongside the message's own range. The
existing message-level resolution keeps working.

Acceptance evidence: a message with a reasoning block and a tool call
resolves each of its rows to the right part; the message-level resolution is
unchanged; the row count and the drawn rows still agree (I1).

### Work package B — The click handler resolves the part

Intent: clicking expands the thing under the cursor.

Required changes: resolve the clicked row to its part and toggle that part;
a row with no expandable part toggles nothing and changes no state.

Acceptance evidence: clicking a tool-call header in a message that also
carries reasoning expands **the tool call**; clicking the reasoning header
expands **the reasoning block**; a second click on the same row collapses
the same part.

### Work package C — Expand-all

Intent: one gesture opens the whole transcript for reading.

Required changes: a new action, bound to `Ctrl+Alt+O` in both maps, that
sets every tool call expanded and every reasoning block collapsed-to-expanded
as appropriate, and toggles the other way on a second press; a palette
command mirroring it; a toast reporting the outcome.

Acceptance evidence: the chord expands every hidden detail in a transcript
containing several tool calls and reasoning blocks; a second press collapses
every one; the palette command does the same; the toast names the outcome;
`Ctrl+O` still opens the workspace dashboard.

### Work package D — The guards and the documents

Intent: the guards' recorded counts match the maps, and the documents
describe the gestures.

Required changes: update the key-authority guards' recorded chord counts and
any model of the binding maps; confirm their `--self-test`s still reject
their defect classes; update `architecture/tui.md`, `docs/tui.md` and the TUI
skill; verify every count they state against its listed source.

Acceptance evidence: the guards pass, their self-tests still reject their
defect classes, and the documents describe the delivered gestures.

## 8. Failure, cancellation, restart, and contention semantics

Not applicable. Frontend-local; no durable state; no concurrent writer.

A row that resolves to no part — a prose row, a separator — must not toggle
anything. Toggling nothing is the correct behaviour, and it must not be
implemented as a fallback to toggling the message.

## 9. Compatibility and migration

- **Rendering and interaction only.** No stored state, no protocol, no
  migration.
- **`Ctrl+O` is unchanged.** The new chord is additive.
- **The click's meaning changes** for messages carrying both a reasoning
  block and a tool call: a click now toggles the part rather than always the
  reasoning. This is the intended fix, and it is recorded as a behaviour
  change.

## 10. Required tests

### Focused unit tests

- The row→part resolution: a message with a reasoning block and a tool call
  resolves each row to the right part; a prose row resolves to no part.
- The click handler: a tool-call row toggles the tool call; a reasoning row
  toggles the reasoning block; a prose row toggles nothing.
- Expand-all: every tool call and reasoning block is expanded; a second press
  collapses every one; the palette command behaves identically; the toast
  names the outcome.
- The binding maps: `Ctrl+Alt+O` is bound in both maps to the same action;
  `Ctrl+O` is still bound to the workspace dashboard in both maps.

### Integration tests

- A rendered transcript containing a tool call and a reasoning block, clicked
  on each header in turn, shows each expanded independently.
- The scrollbar's `total_lines` equals the drawn row count after an
  expand-all (I1) — expanding changes the row count, and the scrollbar must
  follow.

### Restart and recovery tests

None.

### Contention and cancellation tests

None.

### Security and negative tests

None new. Expanding content does not change what the content is.

### Migration and compatibility tests

None.

### Non-vacuity

Every new test must fail against the baseline tree: against the baseline, a
click on a tool-call row in a message that also has reasoning expands the
**reasoning**, and `Ctrl+Alt+O` does nothing.

## 11. Required verification commands

```bash
# the disclosure model and the new action, narrowly
cargo test --locked -p codegg --lib tui::components::messages
cargo test --locked -p codegg --lib tui::input

# the rendered transcript and the click path
cargo test --locked -p codegg --test tui_render

# the key-authority guards, repaired and re-counted
python3 scripts/check_tui_composer_key_authority.py --self-test
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

The guard names above must be confirmed against `ls scripts/check_*` before
running them: the guard set changes, and a command that does not exist is not
evidence.

## 12. Documentation updates

- `architecture/tui.md` — the disclosure model: the row→part resolution, the
  three affordances, and the `Ctrl+Alt+O` gesture.
- `docs/tui.md` — the chord, the click behaviour, and what the collapsed
  view shows.
- `.opencode/skills/tui/SKILL.md` — the gestures, if it lists them.
- `plans/registry.md` — this milestone's row, when it closes.

## 13. Acceptance criteria

- Clicking a tool-call header expands that tool call, in a message that also
  carries a reasoning block.
- Clicking a reasoning header expands that reasoning block.
- Clicking a row that holds nothing expandable changes nothing.
- `Ctrl+Alt+O` expands every hidden detail in the transcript; a second
  press collapses every one; the palette command does the same; the toast
  says which happened.
- `Ctrl+O` still opens the workspace dashboard in both binding maps.
- The key-authority guards pass, with their recorded chord counts updated to
  the new values, and their self-tests still reject their defect classes.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- recording part boundaries would require a second row producer, which would
  break I1;
- a chord is needed that is not free, and re-spelling a live binding would be
  required;
- the guards' chord counts cannot be reconciled with the maps without
  weakening what they enforce;
- scope would expand into keyboard selection of a collapsed call.

## 15. Closure evidence required

- The row→part resolution's and the click handler's tests.
- The expand-all's tests, including the palette command and the toast.
- The binding-map assertion for `Ctrl+Alt+O` and for the unchanged
  `Ctrl+O`.
- The guards' output and their self-tests' results, with the old and new
  chord counts stated.
- The scrollbar's row-count assertion after an expand-all (I1).
- `scripts/verify.sh quick` green at the closing commit.

## 16. Handoff notes

- The `Ctrl+O` binding is load-bearing in Insert mode, per the registry's
  own record. Do not touch it.
- The guards' chord counts are part of their contract. A stale count is a
  closure defect, not a cosmetic drift.
- The click handler's "no part → no toggle" case must be implemented as an
  explicit no-op, not as a fallback to the message-level toggle. A fallback
  would reintroduce exactly the defect this milestone fixes.


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
