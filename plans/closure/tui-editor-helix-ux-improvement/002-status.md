# TUI Editor & Files Panel Helix UX Improvement M002 — Closure Status

Status: closed

Source implementation plan: none. This roadmap's milestones were implemented
from the roadmap's own deliverable boundaries and exit conditions, so the
roadmap **is** the plan and the closure record below is the only evidence
artefact it has.

Source subsystem roadmap:

- `plans/subsystems/tui-editor-helix-ux-improvement-roadmap.md` (M002)

Repository baseline reviewed: `a588a264` (branch `feat/auto-20261020-513a8ce6`)

Applicable ADRs (unchanged by this milestone): none. This milestone adds no
protocol, storage, migration, authorization, or dependency change, and it
reopens no closed milestone of `tui-editor-helix-parity-roadmap.md`.

## 1. What landed

`word_forward`, `word_backward`, and `word_end` resolve against the run
model. `e` lands on a word's **last character** and always advances — a caret
already on a word's last byte moves on to the next word rather than freezing
— and `b` lands on a word's **first character**.

`e` and `b` **select** the word they landed on: the selection's head is the
caret and its anchor is the word's other end, so `b` produces a backwards
selection and the highlighted span is the word either way. `w` is a
positioning motion and does not select — it is the one word motion whose job
is only to move.

Helix's **exclusive-end** landing was rejected, and the deviation is recorded
in `research/tui-editor-helix-ux-improvements-2026-10-20.md` §0.0: CodeGG's
caret rests on a character, and the editor's existing documented contract was
"the last byte of the word". Helix's **category model** and its
**always-advance** rule were adopted unchanged.

## 2. Exit conditions

| Exit condition | Result | Evidence |
|---|---|---|
| a held `e` never repeats an offset | met | `a_word_end_motion_never_stands_still` |
| `e` on a punctuation run advances | met | `a_punctuation_run_is_one_word`, `an_underscore_is_word_content_and_a_punctuation_run_is_a_word_of_its_own` |
| `b` lands on the start of a word | met | `a_word_backward_motion_lands_on_the_start_of_a_word` |
| the auto-selection is asserted at the byte level for `e` and `b` | met | `src/tui/editor.rs` — `apply_motion_selection` / `selected_word_bounds`; `src/tui/commands/editor.rs` — the word-motion trajectory tests |

## 3. Residual risk and open items

The auto-selection is **mode-dependent** by design: outside Select mode a
movement clears the selection, inside it a movement grows it. The rule is
stated in `src/tui/editor.rs` next to `apply_motion_selection` and covered by
`movement_extends_a_char_selection_but_not_a_line_selection`.
