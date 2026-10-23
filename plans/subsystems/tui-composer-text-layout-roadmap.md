# TUI Composer Text Layout Roadmap

Status: conditionally closed

Long-term references:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`

Related ADRs:

- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` (**accepted**
  2026-10-19, with the `space`-chord amendment) — the composer's claim set,
  its fall-through rule and its mode gate. This roadmap **amends** ADR-0042
  part 4's `j`/`k` condition, by the dated-amendment form ADR-0042 itself
  established for its `space` chord. The original Decision text is preserved;
  the amendment is appended.
- `plans/subsystems/tui-main-buffer-text-rendering-roadmap.md` M001 — the
  styled word-aware wrap primitive this roadmap consumes (interface
  dependency).

Authorizing predecessor: `plans/subsystems/tui-composer-helix-parity-roadmap.md`
(active; M001–M007 all closed). That roadmap owns the composer's **key
ownership, text model, motions, selection, clipboard and undo**. This roadmap
owns the composer's **text layout** — how a long line is laid out in rows, how
much of it is visible, and how the caret moves through it. It does not reopen
the predecessor: the predecessor's milestones are closed and their contracts
stand.

Research basis: the repository survey recorded in
`plans/implementation/tui-composer-text-layout/001-composer-word-wrap-and-visual-row-geometry.md`
§3, verified at baseline `f2639b5d2`.

## 1. Purpose and ownership boundary

Make a long prompt legible and reachable in the box it is typed into.

Today a long single-line prompt is **hard-clipped at the right edge**. The box
does not grow, because its height is computed from the number of *newline
characters* in the buffer, and a 500-character line is one newline. The only
way to see the tail is a horizontal scroll that pans the whole paragraph. And
because the height is logical, a long line cannot be scrolled vertically
either: Normal-mode `j`/`k` fall through to the transcript, because the
composer's buffer "has one line".

**This subsystem owns**

- the composer's **visual-row geometry**: how buffer bytes map to wrapped rows,
  and back;
- the composer's **height**: how tall the box is, and when it grows;
- the composer's **scrolling**: what `j`/`k` and the page motions do when the
  content overflows;
- the caret's **cell**: where the caret is drawn, given a wrapped layout.

**This subsystem consumes**

- the styled word-aware wrap primitive from
  `plans/subsystems/tui-main-buffer-text-rendering-roadmap.md` M001;
- the composer's existing text model (`ComposerText`), its byte-offset caret,
  and the shared word/vertical geometry in `src/tui/text_geometry.rs`.

**This subsystem must not own**

- the composer's **key set**. ADR-0042 fixes the thirteen claimed letters;
  this roadmap changes **one condition** on an existing claim and adds no
  letter.
- the composer's **text model**. `ComposerText` keeps its byte-offset caret,
  its selection and its undo history.
- the main buffer's rendering. Owned by
  `plans/subsystems/tui-main-buffer-text-rendering-roadmap.md`.
- the daemon. The composer is a frontend-local string; a layout change is not
  a submission, a revision or a document.

## 2. Decisions

These were answered by the owner on 2026-10-23 and are binding for this
roadmap.

| # | Decision |
|---|---|
| D1 | **`Enter` keeps sending.** `Shift+Enter` inserts a newline. The composer's multi-line support is unchanged; no gesture is re-spelled and no user loses a binding. |
| D2 | The composer's **total height caps at 10 rows** — the mode row, the text rows and the marker rows together. This is a cap on how far the box **expands**, not a limit on how much text may be typed. |
| D3 | Text beyond the cap **remains scrollable**. The 10-row cap bounds the box, not the buffer. |
| D4 | **`Ctrl+u` and `Ctrl+d` page the composer when it overflows**, and page the transcript when it does not. The innermost overflowing surface takes the page. |
| D5 | In Normal mode, **`j` and `k` move the caret by one visual row** when the composer's content does not fit its box. The composer's claim condition changes from "the buffer has more than one line" to "the content does not fit the box". |
| D6 | The **`(multiline)` marker row is kept** for now; it may prove more legible than its absence. Removing it is a later, separate decision. |
| D7 | The composer's **word wrap is by words**, on the same rule the main buffer uses. A word is not broken across rows unless the word alone is wider than the row. |
| D8 | The **pane's height budget wins over the 10-row cap**. The cap is an upper bound on the box, never a claim on the transcript's rows. |

### 2.1 The ADR-0042 amendment, and why it is an amendment

ADR-0042 part 4 states that the composer claims `j` **only when the composer
buffer has more than one line**, and that on a single-line buffer the key must
fall through to transcript scroll unchanged. D5 generalises that condition
from *logical lines* to *fitted content*.

The condition's **intent** is preserved exactly: a composer whose content
fits must not steal the transcript's scroll. Its **letter** cannot be, because
a single logical line can occupy more rows than the box shows, and a condition
that reads "more than one line" would leave that content unreachable.

ADR-0042 already established the form this change takes: its `space`-chord
amendment is a dated `## Amendment:` section appended to the accepted record,
with a table of what changes and what does not, and the original Decision text
left intact. This roadmap follows that precedent rather than superseding the
record, because ADR-0042's own §Supersession says a supersession would
require re-planning the composer's M002 — a cost that buys nothing here, since
one clause changes and the other six parts, the unconditional-claim rule, the
fall-through rule and the mode gate all stand.

The amendment is a **milestone deliverable** (M002), not a precondition: M001
may land the layout without it, and M002 lands the amendment together with the
behaviour it describes.

## 3. Work classification

### Invariants

- **I1 — the composer is a frontend-local string.** A layout change touches
  the model's buffer and nothing else: no daemon call, no revision, no
  document, no submission.
- **I2 — the claim set is fixed.** ADR-0042 part 2 fixes the composer's
  claimed letters. This roadmap adds no row, removes no row, and claims no
  new letter.
- **I3 — a claimed key never falls through.** When the composer claims a key
  in Normal mode it consumes it, whether or not the motion had anywhere to go.
- **I4 — the resolved mode is the gate.** A claim is live only when the
  composer holds the keyboard and the composer's input mode is Normal. The
  raw input-mode field is still never the gate.
- **I5 — the caret is on the byte it names.** The visual-row mapping is a
  bijection between buffer bytes and (row, column) cells. A caret that is one
  row off is a defect, and the closure evidence is a property over widths and
  texts, not a set of examples.
- **I6 — the pane's budget wins.** The composer never takes more than the
  budget allows, whatever the 10-row cap says (D8).
- **I7 — one redraw cadence.** Nothing in this roadmap introduces a second
  frame clock, a second spinner or a free-form status label.

### Capabilities

- **C1 — a long prompt word-wraps and is fully visible.**
- **C2 — the box grows to fit, up to 10 rows.**
- **C3 — the composer scrolls vertically.**
- **C4 — Normal-mode `j`/`k` move by visual row.**
- **C5 — the page motions reach the composer first when it overflows.**

### Infrastructure

- **IN1 — the byte-offset ↔ visual-row mapping.** A small geometry module
  that answers "which row and column is this byte on?" and "which byte is at
  this row and column?" for a wrapped buffer. It is infrastructure: M001 does
  not close on it; it is what makes C1–C3 possible.

### Polish

- **P1 — the horizontal scroll's remaining role.** With wrapping in place the
  horizontal scroll is needed only for a single token wider than the row. The
  milestone must state whether it stays, and under what condition.

## 4. Non-goals

- **The main buffer's rendering.** Owned by
  `plans/subsystems/tui-main-buffer-text-rendering-roadmap.md`.
- **The editor's soft wrap.** The editor has no soft wrap today; it is a
  separate surface with its own geometry.
- **Re-spelling `Enter`.** D1 — no gesture changes.
- **Adding a composer verb.** No new letter, no new chord, no new verb. D5
  changes the *condition* on an existing claim.
- **Changing the composer's text model, selection, clipboard or undo.**
- **Changing the 40 % pane budget.** The composer grows inside it; the budget
  is not this roadmap's to spend.
- **A configuration key for the row cap.** The cap is a product decision with a
  recorded owner decision (D2), not a tunable. A future configurability
  question is separate work.

## 5. Current state

Verified at baseline `f2639b5d2`.

### 5.1 There is no wrapping, and the paragraph is clipped

The composer's render builds one `Line` per **logical** line and paints them
with a paragraph that has **no wrap**. A ratatui paragraph without a wrap
truncates each line at the area width, so a long prompt is cut at the right
edge. The only recourse is a horizontal scroll, which is the paragraph's x
offset and therefore pans **every** line together.

### 5.2 The height is logical, and the ceiling is a pane budget, not a row cap

The height is computed as one mode row, plus the number of **logical** lines,
plus a character-count row and a multiline-marker row, clamped to a minimum of
three rows and a maximum that is **40 % of the pane's height**. So a
500-character single-line prompt is a three-row box with its tail invisible,
and the maximum is a share of the pane rather than a number of rows.

### 5.3 The geometry is logical everywhere

The height, the scroll clamp, the caret's visibility, the caret's cell and the
vertical motions all count **logical lines**. The caret's column is derived
from the byte offset within its logical line, and the horizontal scroll is
applied to the whole paragraph.

### 5.4 `j`/`k` are conditional on a logical line count

The composer claims `j` and `k` when its buffer has more than one line. On a
single-line buffer they fall through to transcript scroll — which is ADR-0042
part 4's requirement and the reason the condition exists.

### 5.5 The width at height-computation time is available

The session layout splits the pane **vertically** into a header, the main
content, the prompt chunk and a footer. The prompt chunk's width is therefore
the pane's width, known **before** the height is computed. The height/width
dependency is not circular, which is the risk that could have made a wrapped
composer unimplementable.

### 5.6 The page motions reach the transcript only

`Ctrl+u` and `Ctrl+d` are bound to the page-up and page-down actions, which
reach the transcript's scroll directly. There is no composer page motion.

## 6. Target architecture

One geometry, consumed by every measurement of the composer.

```
   ComposerText (bytes, selection, undo)
          │
          ▼
   wrap_styled_spans_by_word(spans, width)   ← from the main-buffer roadmap M001
          │
          ▼
   rows: Vec<Row>          each row carries its byte range
          │                 │
          │                 └──▶ byte ↔ (row, column)   (IN1)
          ▼
   ┌──────────────────────────────────────────────┐
   │ height  = 1 + min(rows, 10) + marker rows    │   ≤ 40 % of the pane
   │ scroll  = visual row offset                  │
   │ caret   = row + column of the caret's byte   │
   │ visible = rows[scroll .. scroll + visible]   │
   └──────────────────────────────────────────────┘
```

Every count that used to be a logical line becomes a **visual row**, and the
one mapping that makes that possible is a single module with a single
responsibility.

## 7. Dependency graph

```text
main-buffer M001 ──(interface)──▶ M001 ──(hard)──▶ M002
```

- **main-buffer M001 → M001** is an *interface* dependency: M001 consumes the
  styled word-aware primitive. The primitive's signature is fixed by that
  milestone's plan, so this roadmap does not wait on an undecided shape.
- **M001 → M002** is a *hard* dependency: M002's `j`/`k` condition and the
  ADR-0042 amendment both describe a wrapped, overflow-aware composer, and
  the amendment's text would be false against M001's layout.

## 8. Milestones

### Milestone M001 — The composer word-wraps, grows, and scrolls

**Class:** capability (C1, C2, C3), with an infrastructure obligation (IN1).

**Objective:** a long prompt wraps by words, the box grows to fit up to 10
rows, and the content beyond the cap is reachable by scrolling.

**Dependencies:** the main-buffer roadmap's M001 (interface).

**Deliverable boundary:** the composer renders wrapped rows; the height is
computed from the wrapped row count at the pane's width; the scroll offset is
a visual row offset; the caret's cell is the (row, column) of the caret's
byte.

**User or operator value:** a long prompt is fully visible and correctly
formatted, instead of being cut at the right edge.

**Exit conditions:**

- A prompt longer than the box's width wraps at word boundaries and every
  character of it is reachable.
- The box grows with the content and stops at 10 rows; text beyond that is
  scrollable (D2, D3).
- The 40 % pane budget still wins over the 10-row cap (I6, D8).
- The caret's drawn cell is the cell of the caret's byte, for a set of widths
  and texts — a property over cases, not a set of examples (I5).
- Scrolling the composer keeps the caret visible.
- The composer's existing motions, selection, clipboard and undo are
  unaffected, and the existing tests that assert the logical-line height are
  either rewritten to the visual-row contract or explicitly kept as the
  logical-line contract they actually test.

**Deferred work:** the page motions and Normal-mode scrolling (M002); the
horizontal scroll's remaining role (P1).

### Milestone M002 — Normal-mode scrolling and the page motions

**Class:** capability (C4, C5).

**Objective:** in Normal mode, `j` and `k` move the caret by a visual row when
the composer's content does not fit; the page motions reach the composer
first when it overflows.

**Dependencies:** M001 (hard).

**Deliverable boundary:** the composer's claim condition for `j`/`k` becomes
"the content does not fit the box"; the vertical motions are by visual row;
`Ctrl+u` and `Ctrl+d` page the composer when it overflows and the transcript
otherwise; ADR-0042 carries the dated amendment.

**User or operator value:** the composer is navigable with the same keys the
user already knows, and a page gesture reaches the surface the user is
actually looking at.

**Exit conditions:**

- On a composer whose content fits, `j` and `k` still fall through to
  transcript scroll, unchanged (ADR-0042 part 4's intent, preserved).
- On a composer whose content does not fit, `j` and `k` move the caret by one
  visual row and are consumed by the composer (I3).
- The condition that decides this is the composer's **fitted content**, not
  the buffer's newline count.
- `Ctrl+u` and `Ctrl+d` page the composer when it overflows and the
  transcript otherwise, and the transcript's page behaviour is unchanged when
  the composer fits.
- **ADR-0042 carries a dated amendment** whose Decision text is preserved
  intact and whose amendment table names exactly what changes and what does
  not.
- The composer key-authority guard's known condition list carries the new
  condition, and the guard's `--self-test` still rejects its defect class.
- The help overlay's `j`/`k` entry states the new condition truthfully.

**Deferred work:** per-row composer selection in Normal mode.

## 9. Cross-cutting requirements

### Storage and migration

None. The composer's buffer is frontend-local and unchanged.

### Protocol and compatibility

None. No daemon call, no protocol, no revision (I1).

### Security and authorization

None new. The composer is a frontend-local string that is delivered to the
daemon only on submission, and this roadmap changes neither.

### Concurrency, cancellation, and recovery

Not applicable. The composer is frontend-local; there is no concurrent writer
and nothing to recover.

### Observability and audit

None required.

### Performance and resource use

- The wrap runs on the render path. It must be bounded by the composer's
  content and must not be re-run per frame without cause; the existing
  redraw cadence (I7) is unchanged.
- The visual-row mapping is computed from the wrapped rows, so a very long
  prompt costs one wrap per layout, not one per keystroke beyond what the
  existing render already does. The closure record should state the measured
  cost rather than assert it.

### Documentation and operations

- `architecture/tui.md` — the composer's layout contract, the wrap rule, the
  height cap and the overflow behaviour.
- `docs/tui.md` — the user-facing description of the box's growth, the cap,
  the scroll and the `Ctrl+u`/`Ctrl+d` behaviour.
- `.opencode/skills/tui/SKILL.md` — the same contract for the module guide.
- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` — the dated
  amendment (M002).
- `plans/registry.md` — this roadmap's row and its milestone rows.

## 10. Verification strategy

- **Focused unit tests** for the geometry module: byte ↔ (row, column) for a
  set of widths and texts, including an empty buffer, a buffer ending in a
  newline, a token wider than the row, and multi-byte characters.
- **Focused unit tests** for the height: one row of text is one row of box;
  the box grows with the content; the box stops at 10 rows; the 40 % budget
  still binds on a short pane.
- **Focused unit tests** for the wrap: word boundaries, a token wider than the
  row, and the OSC-8 exclusion inherited from the primitive.
- **Focused unit tests** for the claim condition: a composer that fits falls
  through; a composer that overflows is claimed.
- **Focused unit tests** for the page motions: a composer that overflows takes
  the page; one that fits leaves the transcript's page behaviour unchanged.
- **The existing composer tests** that assert the logical-line height, the
  horizontal cursor visibility and the scroll clamp are reviewed one by one:
  each is either rewritten to the visual-row contract or kept as the
  logical-line contract it actually tests, with the disposition recorded.
- **A non-vacuity check** on every new test: each must fail against the
  baseline tree.
- **No new static guard** is required. The composer key-authority guard is
  **repaired** — its known-condition list carries the new condition — and its
  `--self-test` must still reject its defect class.

## 11. Risks and decision points

| # | Risk | Disposition |
|---|---|---|
| R1 | **The byte-offset ↔ visual-row mapping is the correctness core.** One wrong row means the caret and the text disagree. | I5 makes it an invariant, and the closure evidence is a property over widths and texts rather than examples. |
| R2 | **The height/width dependency could have been circular.** It is not: the prompt chunk's width is the pane's width, known before the height is computed. | Recorded as evidence in §5.5. The implementation must still compute the wrap width from the **pane** width, not from a not-yet-existing chunk. |
| R3 | **The claim condition changes an accepted ADR's letter.** | D5 and §2.1: a dated amendment appended to ADR-0042, following ADR-0042's own precedent, with the original Decision text preserved. The alternative — superseding the record — would trigger the re-planning of the composer's closed M002 for a one-clause change. |
| R4 | **The 10-row cap could squeeze the transcript.** | I6 and D8: the 40 % budget wins. On a 25-row pane the budget is exactly 10 rows; on a 15-row pane it is 6. The cap is an upper bound, never a claim. |
| R5 | **The guard's known-condition list is a closed set.** A new condition name is a change to an existing guard's model, and a guard that rejects the new condition would fail the milestone for the wrong reason. | M002 names the guard repair explicitly, and its `--self-test` must still reject its defect class. No new guard is added. |
| R6 | **The horizontal scroll may become dead.** With wrapping, a horizontal scroll is only needed for a token wider than the row. | P1: M001 must state whether it stays and under what condition, rather than leaving a dead field in place. |
| R7 | **The `(multiline)` marker row is kept by decision, not by evidence.** D6 keeps it "for now". | Kept in M001. If a later milestone finds it redundant, that removal is its own decision with its own evidence — recorded here so it is not silently absorbed. |
| R8 | **The main-buffer roadmap's primitive signature is consumed here.** | The interface dependency is named in §7 and the primitive's signature is fixed in that milestone's plan. |

No decision in this roadmap requires a new ADR: the one durable decision it
touches is an amendment to ADR-0042, which ADR-0042's own precedent covers.

## 12. Completion definition

This roadmap is closed when:

- M001 and M002 each have an accepted closure record;
- a long prompt wraps by words, is fully reachable, and the box grows to at
  most 10 rows and scrolls beyond that;
- the caret is always drawn on the cell of its byte;
- `j`/`k` and the page motions behave as D4 and D5 specify, and a composer
  that fits still leaves the transcript's scroll alone;
- ADR-0042 carries the dated amendment with its original Decision text
  intact;
- I1–I7 hold and are evidenced;
- `architecture/tui.md`, `docs/tui.md` and `.opencode/skills/tui/SKILL.md`
  describe the delivered contract, and their stated counts are verified
  against their listed sources;
- the registry records this roadmap's closure and the unblock audit that
  followed it.

## 13. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 | conditionally closed | `plans/implementation/tui-composer-text-layout/001-composer-word-wrap-and-visual-row-geometry.md` | `plans/closure/tui-composer-text-layout/001-status.md` | main-buffer M001 (interface) — satisfied: the primitive's signature was fixed there and the composer consumed it |
| M002 | conditionally closed | `plans/implementation/tui-composer-text-layout/002-normal-mode-scrolling-and-page-motions.md` | `plans/closure/tui-composer-text-layout/002-status.md` | M001 (hard) |

**Outcome (2026-10-23).** M001 and M002 are implemented and locally verified
in the working tree, and each carries a closure record. The line is
**conditionally closed**: the implementation lands in a commit on the branch
that carries these records, and the **hosted `CI / verify` lane has not run
against that commit** — a CI run cannot precede the commit it runs against, so
the lane is the one named outstanding evidence, and each closure record states
it.

What the line delivered: the composer **wraps by words** instead of clipping at
the right edge; the box **grows** with the text to a **10-row total cap** inside
the **40 % frame budget** (the budget wins over the cap, and text past the cap
stays scrollable); the scroll and the caret are **visual-row** operations, with
the caret's drawn cell computed from its **byte**; `j`/`k` are claimed only when
the composer's content **does not fit its box** — the exact-fit case falls
through to the transcript — and move the caret one visual row; and `Ctrl+u` /
`Ctrl+d` page the composer when it overflows and the transcript otherwise.

**ADR-0042 was amended in place, dated 2026-10-23**, before any of this landed:
part 4's `j`/`k` condition generalises from the buffer's **logical** line count
to the composer's **fitted content**, preserving the condition's intent exactly
while changing its measure. The implemented condition matches the amendment's
text **word for word** — the closure record quotes both. The ADR itself was not
edited.

One **behaviour change** is recorded rather than hidden: the mode row and the
marker rows are now **pinned** in the composer box while the text scrolls
between them. Before, they scrolled away with the text. A second consequence is
stated in the closure record: the page is **not mode-gated**, so in Insert mode
a `Ctrl+d` on an overflowing composer moves the caret — the same shape as vim's
`Ctrl+f`/`Ctrl+b`.
