# TUI Composer Text Layout Milestone M002 — Normal-Mode Scrolling and the Page Motions

Status: implemented — closure record at `plans/closure/tui-composer-text-layout/002-status.md`

Repository baseline: `f2639b5d2` (`origin/main`)

Source roadmap: `plans/subsystems/tui-composer-text-layout-roadmap.md#milestone-m002--normal-mode-scrolling-and-the-page-motions`

Long-term requirements:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`

Applicable ADRs:

- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` (**accepted**
  2026-10-19, **amended 2026-10-23** with
  [§ Amendment: the wrapped visual row](#amendment-the-wrapped-visual-row)) —
  the composer's claim set, the mode gate, the fall-through rule, and the
  amended part 4 condition. **This milestone implements the amendment.** The
  amendment's text is already written and is the contract; this milestone
  realises it and must not restate it differently.

Primary class: capability (roadmap C4, C5)

## 1. Objective

In Normal mode, `j` and `k` move the composer's caret by one **visual row**
when the composer's content does not fit its box — and fall through to
transcript scroll when it does. The page motions reach the composer first
when it overflows.

## 2. Why this milestone is ready

- **M001 (hard)** is closed: the composer wraps, the box grows, and the
  visual-row geometry exists. The amended condition in ADR-0042 describes a
  layout that now exists.
- **The decision is already recorded.** ADR-0042's dated amendment states the
  condition, the motion, the fall-through rule and the consequence. The
  implementation is the only thing left.
- **The guard exists and is armed.** The composer key-authority guard polices
  the claim table against the binding maps today, and its known-condition set
  carries the composer's conditions. The milestone **repairs** that guard for
  the new condition; it adds no guard.

## 3. Current implementation evidence

Verified at baseline `f2639b5d2`.

### 3.1 The claim condition is a logical-line count

The composer's `j` and `k` claims are gated on a condition that holds when the
buffer has more than one line. The gate is evaluated at the app's key
dispatch, before the binding map is consulted, and the condition's value is
the buffer's logical line count.

### 3.2 The motions move by logical line

The composer's vertical motion is the shared `move_vertical`, which moves an
offset by a number of **logical** lines, preserving the column where the
target line allows and clamping at the last line.

### 3.3 The page motions reach the transcript only

`Ctrl+u` and `Ctrl+d` are bound to the page-up and page-down actions in both
binding maps. Those actions reach the transcript's scroll directly — the
transcript's own page-up and page-down, whose page size is the viewport height
minus two.

There is no composer page motion, and no surface-priority rule between the
composer and the transcript for a page gesture.

### 3.4 The panels already have the shape this milestone needs

The page actions already branch on which surface is active — the workspace
dashboard takes the page when it is up, and the transcript takes it
otherwise. The composer follows the same "innermost surface first" shape.

### 3.5 The guard's condition set is closed

The composer key-authority guard carries a known-condition set, so a new
condition name must be added to it. The guard's `--self-test` injects a
defect and proves the guard rejects it; that self-test must still reject its
defect class after the change.

### 3.6 The help overlay states the old condition

The help overlay's `j`/`k` entry carries a condition note describing the
composer as claiming those keys when its buffer has more than one line. After
the amendment that note is false, and the help overlay's condition notes are
the user-facing statement of the condition.

## 4. Invariants that must not regress

- **I1 — the composer is a frontend-local string.** A motion and a page
  gesture touch the model's buffer and the view; neither is a daemon call, a
  revision, a document or a submission.
- **I2 — the claim set is fixed.** ADR-0042 part 2 fixes the composer's
  claimed letters. This milestone adds no row, removes no row, and claims no
  new letter.
- **I3 — a claimed key never falls through.** When the composer claims a key
  in Normal mode it consumes it, whether or not the motion had anywhere to go.
- **I4 — the resolved mode is the gate.** A claim is live only when the
  composer holds the keyboard and the composer's input mode is Normal.
- **I5 — the caret is on the byte it names.** A visual-row motion must land
  the caret on a byte, and the caret's drawn cell must be that byte's cell.
- **I6 — the pane's budget wins.** Unchanged.
- **I7 — one redraw cadence.** Unchanged.

## 5. Scope

### In scope

- The composer's `j`/`k` claim condition: **the content does not fit the box**,
  measured as the wrapped row count exceeding the visible row count.
- The `j`/`k` motions move the caret by one **visual row**.
- `Ctrl+u` and `Ctrl+d` page the **composer** when it overflows, and the
  transcript when it does not.
- The composer key-authority guard's known-condition set carries the new
  condition, and its self-test still rejects its defect class.
- The help overlay's `j`/`k` condition note states the new condition.
- The user-facing documentation states the new condition.

### Explicitly out of scope

- ADR-0042's amendment text. It is **already written**; this milestone
  implements it and must not restate it differently.
- Any change to the claim set, the mode gate, or the fall-through rule.
- Re-spelling any key.
- The wrap, the height and the geometry (M001).
- The main buffer's rendering, and the main buffer's own `j`/`k` behaviour,
  which is unchanged.

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

**The claim condition.** The condition that gates `j` and `k` becomes "the
composer's content does not fit its box". It is evaluated against the
**fitted** content — the wrapped row count against the visible row count —
and **not** against the buffer's newline count.

The distinction matters in both directions, and ADR-0042's amendment states
it explicitly: a composer whose text wraps to three rows in a three-row box
**fits**, and `j` must fall through to the transcript; a composer whose text
wraps to four rows in a three-row box does **not** fit, and `j` moves the
caret a visual row.

**The visual-row motions.** The composer's `j` and `k` verbs move the caret
by one visual row, using M001's geometry module, and the caret lands on a
byte. The desired column is carried across the move where the target row
allows, matching the shared geometry's existing rule.

**The page motions.** `Ctrl+u` and `Ctrl+d` page the composer when the
composer's content overflows its box, and the transcript otherwise. The
transcript's page behaviour is unchanged when the composer fits.

The page size follows the same rule the transcript uses — the visible height
minus two, with a floor of one — so the two surfaces page comparably.

**The guard.** The composer key-authority guard's known-condition set carries
the new condition name, and its model of the claim table understands the
condition's meaning. Its `--self-test` must still inject a defect and prove
the guard rejects it. **The guard is repaired, not extended by a new guard.**

**The help overlay and the docs.** The `j`/`k` entry's condition note states
the new condition truthfully.

### Security and authorization

None. Both gestures are frontend-local.

### Documentation and static guards

- `architecture/tui.md` — the composer's overflow behaviour, the claim
  condition, and the page-motion priority.
- `docs/tui.md` — the `j`/`k` condition and the `Ctrl+u`/`Ctrl+d` behaviour.
- `.opencode/skills/tui/SKILL.md` — the same, if it describes the gestures.
- **No new static guard.** The composer key-authority guard is **repaired**,
  and its self-test must still reject its defect class.

## 7. Ordered work packages

### Work package A — The condition and the motions

Intent: `j`/`k` move by visual row exactly when the content does not fit.

Required changes: add the fitted-content condition to the composer's claim
conditions; evaluate it from the wrapped row count and the visible row count;
move the vertical motions onto the visual-row geometry; carry the desired
column where the target row allows.

Acceptance evidence: on a composer whose content fits, `j`/`k` fall through
to transcript scroll, unchanged; on a composer whose content overflows, `j`
and `k` move the caret by one visual row and are consumed; a composer whose
text wraps to exactly the box's height **fits** and falls through.

### Work package B — The page motions

Intent: the page gesture reaches the surface the user is looking at.

Required changes: `Ctrl+u` and `Ctrl+d` page the composer when it overflows
and the transcript otherwise; use the same page-size rule the transcript uses.

Acceptance evidence: a composer that overflows takes the page; one that fits
leaves the transcript's page behaviour unchanged; the transcript's own page
size is unchanged.

### Work package C — The guard, the help overlay and the documents

Intent: the guard understands the new condition, and the user-facing text
states it truthfully.

Required changes: add the condition to the guard's known-condition set and
teach its model the condition's meaning; confirm the self-test still rejects
its defect class; update the help overlay's `j`/`k` condition note; update
`architecture/tui.md`, `docs/tui.md` and the TUI skill; verify every count
they state against its listed source.

Acceptance evidence: the guard passes; its self-test still rejects its defect
class; the help overlay's note matches the implemented condition.

### Work package D — The ADR's landing

Intent: the recorded amendment and the shipped behaviour agree.

Required changes: none to ADR-0042 — its amendment is already written.
Confirm that the implemented condition, the motion and the fall-through rule
match the amendment's text word for word; if any of them does not, **stop and
report** rather than amending the record to fit the code.

Acceptance evidence: the closure record states that the implemented behaviour
matches the amendment, quoting the condition from the code.

## 8. Failure, cancellation, restart, and contention semantics

Not applicable. Frontend-local; no durable state; no concurrent writer.

The gate is evaluated before the binding map is consulted, so a claim that
holds consumes the key whether or not the motion had somewhere to go (I3). A
composer with no room to move must still consume the key.

## 9. Compatibility and migration

- **Interaction only.** No stored state, no protocol, no migration.
- **No key is re-spelled.** The binding maps' existing `Ctrl+u`/`Ctrl+d`
  bindings stay; only their target changes when the composer overflows.
- **A behaviour change for one case.** Today, on a composer whose text is a
  single long line, `j`/`k` scroll the transcript. After the change they move
  the composer's caret. That is the point of the milestone, and it is
  recorded as a behaviour change rather than a neutral refactor.
- **The fall-through case is preserved.** A composer whose content fits is
  unchanged.

## 10. Required tests

### Focused unit tests

- The condition: a composer whose content fits does not hold the condition; a
  composer whose content overflows holds it; a composer whose text wraps to
  exactly the box's height **fits**.
- The motions: `j` moves the caret one visual row down and `k` one up; the
  caret lands on a byte and its drawn cell is that byte's cell; the caret
  clamps at the first and last visual rows.
- The page motions: an overflowing composer takes the page; a fitting
  composer does not; the transcript's page behaviour is unchanged when the
  composer fits.

### Integration tests

- A long prompt in the composer: `j` moves the caret down a wrapped row
  rather than scrolling the transcript.
- `Ctrl+d` scrolls the composer when it overflows and scrolls the transcript
  when it does not.
- The composer key-authority guard's self-test still rejects its injected
  defect.

### Restart and recovery tests

None.

### Contention and cancellation tests

None.

### Security and negative tests

None new.

### Migration and compatibility tests

None.

### Non-vacuity

Every new test must fail against the baseline tree: against the baseline, `j`
on a single-long-line composer scrolls the transcript rather than moving the
caret, and `Ctrl+d` never reaches the composer.

## 11. Required verification commands

```bash
# the claims, the motions and the guards, narrowly
cargo test --locked -p codegg --lib tui::composer_claims
cargo test --locked -p codegg --lib tui::components::prompt
cargo test --locked -p codegg --lib tui::text_geometry

# the rendered composer and the key path
cargo test --locked -p codegg --test tui_render

# the composer claim guard, repaired and re-proved
python3 scripts/check_tui_composer_key_authority.py --self-test

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

- `architecture/tui.md` — the composer's overflow behaviour, the claim
  condition, and the page-motion priority.
- `docs/tui.md` — the `j`/`k` condition and the `Ctrl+u`/`Ctrl+d` behaviour.
- `.opencode/skills/tui/SKILL.md` — the gestures, if it lists them.
- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` — **the
  amendment is already written.** This milestone confirms the implementation
  matches it; it does not rewrite it.
- `plans/registry.md` — this milestone's row, when it closes.

## 13. Acceptance criteria

- On a composer whose content fits its box, `j` and `k` still scroll the
  transcript, unchanged.
- On a composer whose content overflows its box, `j` and `k` move the caret
  by one visual row and are consumed by the composer.
- The condition is evaluated against the fitted content, not against the
  buffer's newline count: a composer whose text wraps to exactly the box's
  height falls through.
- `Ctrl+u` and `Ctrl+d` page the composer when it overflows and the
  transcript otherwise.
- The composer key-authority guard passes, carries the new condition, and its
  self-test still rejects its defect class.
- The help overlay's `j`/`k` note states the new condition.
- The implemented behaviour matches ADR-0042's dated amendment.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- the implemented condition cannot be made to match the amendment's text,
  rather than the amendment being adjusted to fit the code;
- the guard cannot carry the new condition without weakening what it
  enforces;
- a key would have to be re-spelled;
- the claim set, the mode gate or the fall-through rule would have to change;
- scope would expand into the wrap, the height or the geometry (M001), or
  into the main buffer.

## 15. Closure evidence required

- The condition's and the motions' unit tests, including the exact-fit case.
- The page motions' tests.
- The composer key-authority guard's output and its self-test's result.
- The help overlay's condition note, quoted.
- The implemented condition quoted from the code, next to ADR-0042's
  amendment, with a statement that they agree.
- A statement that no key was re-spelled and that the claim set is unchanged
  (I2).
- `scripts/verify.sh quick` green at the closing commit.

## 16. Handoff notes

- ADR-0042's amendment is the contract. Read it before writing the
  condition; the exact-fit case is in it and is easy to get backwards.
- The gate runs **before** the binding map is consulted. A claim that holds
  must consume the key even when the motion has nowhere to go.
- The guard's known-condition set is a closed list. Adding a condition is a
  change to the guard's model, and the guard's self-test must still reject
  its defect class afterwards — otherwise the milestone is green for the
  wrong reason.
- The page-motion priority mirrors the workspace dashboard's existing branch
  shape. Follow that shape rather than inventing a second one.
