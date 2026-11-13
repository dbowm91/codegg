# Five-Line Closure Corrective — Owner Decisions

Status: open — seven decisions, none taken

Source: `plans/closure/five-line-closure-corrective/001-status.md` §9, which
lists these as questions. This memo gives each one its options, what each costs,
and a recommendation. Nothing here is a decision; a decision is an owner
sentence recorded against the record named in each heading.

Why they are here rather than resolved: every one of them is a **scope**
question — is this condition met, or is the work not done — and none can be
settled by a run. The gate class they used to sit beside is discharged: `full`
is green on the tree, and the sandbox narrowing's evidence is the hosted run
recorded in the closure record, not a local claim.

---

## 1. `tui-modularity/001` — is the structural reading of criterion 6 acceptable?

**The criterion.** "`git diff` shows no change under `src/tui/**` or
`src/theme/**`" — M001 was to add a measurement harness without touching what it
measures.

**Why it cannot be read literally.** M001 was implemented in a worktree shared
with M002, M003 and four other lines, so **no M001-only diff exists**.
`src/theme/**` is clean tree-wide; `src/tui/**` carries 21 changed or new paths
from other milestones. The tree is now also one commit, so a per-milestone diff
cannot be reconstructed without rewriting history.

**Options.**

| | Option | Cost | Benefit |
|---|---|---|---|
| A | Accept the structural reading: "no M001 artifact is a production file; the harness exists and is wired into both lanes" | The criterion is satisfied by argument, not by a diff | True today: the harness lives under `tests/tui_render/` and `scripts/`, and no production file was needed. Closes the record |
| B | Require the diff: land milestones separately and re-read | Impossible retrospectively; would need a rebase that changes published history for no new evidence | Would give a mechanical reading |
| C | Amend the criterion to what it protects: "no production rendering file changed as a consequence of M001" | Amends an acceptance criterion after the fact | Names the property that actually mattered |

**Recommendation: A + C.** The property criterion 6 protects — that the harness
is additive, not a production rewrite — is exactly what the structural reading
measures, and C makes that explicit so the next reader is not asked to
re-litigate it. B is not merely expensive: it is now unavailable without a
rebase, so if the owner wanted B, that is the fact to weigh.

---

## 2. `tui-modularity/008` — is 8 of 10 the accepted end state for criterion 4?

**The criterion.** "The ten `Option<Rect>` fields are gone and hit-testing reads
the plan." Eight are retired; `dialog_area` (`app/mod.rs:382`) and
`sidebar_area` (`:437`) survive.

**Why they survive.** The attribution to `src/tui/commands/**` is **correct**:
`commands/panels.rs` reads `App::sidebar_area` at ten production sites, and
`interactive_terminal.rs:3262` reads `App::dialog_area` from a test helper.

**Options.**

| | Option | Cost | Benefit |
|---|---|---|---|
| A | Accept eight, amend criterion 4 and roadmap §11 item 6 to say eight | Frame geometry keeps two owners; the carve-order invariant is tested for the frame's regions but not for the dialog box or the sidebar column | Zero further work; the record closes |
| B | Finish both: `dialog_area` is already a single derivation at `dialog_rect`; `sidebar_area` moves four readers | Bounded but real: the sidebar touches the command family plus three mouse paths, with render-corpus movement | One geometry owner; criterion met literally |

**Recommendation: A, with the two named as a follow-up milestone rather than a
condition.** The value criterion 4 protects is 80% delivered and the remaining
20% is not a correctness risk today — both fields are *read*, never written, by
the command family. A `conditionally closed` record that carries an undecided
question forever is worse than a bounded follow-up with its own evidence. If the
owner prefers B, do `dialog_area` first: it is a single derivation and its move
is the cheap half.

---

## 3. `tui-modularity/003` — the `permission.rs` title-badge exception

**The criterion.** Every dialog surface adopts the one `Section` frame. **39 of
40** do; `permission.rs` keeps a hand-built frame because its title row carries a
second, right-aligned, risk-coloured badge that `Section` cannot express.

**Why it is not laziness.** Folding the badge into the title loses the alignment
and the only `ToolRisk`-coloured paint in the TUI; moving it into the body adds a
content row to every permission prompt. The exception is documented at
`architecture/tui.md:4199-4216` and `src/tui/components/dialogs/permission.rs:125-153`.

**Options.**

| | Option | Cost | Benefit |
|---|---|---|---|
| A | Amend criterion 1 to "every dialog surface whose title row needs no adjunct" | The criterion becomes conditional: a future surface with an adjunct is out of scope by definition | Honest, and closes the record |
| B | Scope a `Section` title adjunct — a second, independently styled title span — for a follow-up and adopt it in `permission.rs` | A primitive change with 40 call sites to re-verify and corpus movement | The criterion is met literally and the primitive can express what one caller needs |
| C | Accept the exception permanently, as documented | The primitive is knowingly incomplete | Zero work; the exception is where a reader looks |

**Recommendation: A now, B as a follow-up.** The badge is the only
`ToolRisk`-coloured paint on screen; losing it to satisfy a mechanical criterion
is the wrong trade. But the gap is real — a second caller will hit it — so it
belongs in the primitive's backlog, not in this record's conditions.

---

## 4. `tui-modularity/002` and `/003` — is the corpus diff-note substitute acceptable?

**The artifact.** The per-panel and per-dialog corpus diff note — which cells
moved when a surface adopted `Section` — **does not exist**, and the
pre-adoption frames are no longer renderable. The plan requires the substitute
to be accepted in writing rather than invented.

**The substitute.** The whole `tui_render` family green (1237/0), the
`dialog_dispatch_coverage` measurement unmodified and green, and a hosted
content-row assertion.

**Options.**

| | Option | Cost | Benefit |
|---|---|---|---|
| A | Accept the substitute | A body change *alongside* an edge change could pass every current assertion | Mechanically checkable, and it is the only available evidence |
| B | Produce the artifact retroactively | Impossible: the frames are gone | — |
| C | Accept, and strengthen it: a digest of the corpus so "byte-identical" is a hash, plus the hosted content-row assertion | Bounded; one test and one recorded digest | The next milestone's claim becomes mechanical rather than asserted |

**Recommendation: A + C.** A is the only available evidence; C is cheap and
converts "asserted" into "measured" for every future adoption.

---

## 5 and 6. `helix/001` and `/002` — the `space ?` keymap box omits `_` and `Alt-d`

**The state.** `EDITOR_KEYMAP` (`src/tui/components/editor.rs:253`) is a 26-row
list. Its delete row reads `d / y` and its change row `c / Alt-c`;
`docs/tui.md` carries all four spellings. The box is where a user learns the
verbs in the editor; `docs/tui.md` is the complete document.

**Options.**

| | Option | Cost | Benefit |
|---|---|---|---|
| A | Accept as non-gating: the box is a condensed aid, the docs are complete | A reader who learns from the box never discovers trim or the no-yank delete | No code change, no corpus movement |
| B | Add the two spellings: `d / Alt-d / y` on the existing row and one new `_` row | One row added and one extended; the corpus moves where the box is captured | The box and the docs agree; the two Helix verbs a user reaches for become discoverable |
| C | B plus a doc-drift guard so the box cannot diverge from the keymap again | A guard and a canonical source | This class of drift cannot recur |

**Recommendation: B, then C if cheap.** There is no fixed row count — the list
is scrollable — so B is one row and one edit. The omission is a discoverability
defect, not a documentation preference: `_` (trim) and `Alt-d` (no-yank delete)
are exactly the verbs a Helix user looks for, and the box is where they look.

---

## 7. `helix/009` — ratify the single-chain pruning rule?

**The rule as implemented.** Pruning a single unbranched chain drops the **oldest
node** and re-parents its one child to the root (`editor.rs:7136-7157`,
`:7164-7191`). The plan's work package D said "an interior node is never dropped
while a child survives". The implementation contradicts that wording — and keeps
a 257-edit session from either clearing its whole history or failing the
`len == MAX_EDITOR_UNDO_DEPTH` linear-equivalence gate.

**A second divergence in the same area.** Ordinary leaf-on-leaf pruning is
**silent**; only the fallback clear is disclosed (`history_bound_cleared` paints
"history cleared at its bound"). The plan said pruning "discloses".

**Options.**

| | Option | Cost | Benefit |
|---|---|---|---|
| A | Ratify as implemented | An interior node can be dropped, so the oldest surviving undo point is re-parented: the structure above it survives, its identity does not | Bounded memory, no history clearing, the linear-equivalence gate holds |
| B | Revert to the plan's wording | On a single chain at the bound, either the history clears (the plan's own fallback) or the gate fails | The stated invariant holds literally |
| C | Ratify A **and** disclose the first prune | A one-shot notice; a permanent latch would be noise after 256 edits | The user learns their history was trimmed, which the plan required |

**Recommendation: A + C.** The implemented rule is the only one that both keeps
the gate and never clears a 257-edit history; the plan's wording was written
before that gate existed. Silence is the part that should change: one
disclosure when pruning first occurs.

---

## What each answer unblocks

| Answer | Effect |
|---|---|
| 1 = A | `tui-modularity/001` → `closed` |
| 2 = A | `tui-modularity/008` → `closed`; two fields named as a follow-up |
| 3 = A | `tui-modularity/003` → `closed`; the title adjunct becomes a follow-up |
| 4 = A | `tui-modularity/002` and `/003` lose their last open condition |
| 5/6 = A | `helix/001` and `/002` → `closed` |
| 5/6 = B | Two rows added; both records → `closed` after a corpus re-bank |
| 7 = A | `helix/009` → `closed`; C adds a disclosure |

With all seven answered, the five lines read **all `closed`** except where a
follow-up was chosen — and the cross-line corrective closes with them.
