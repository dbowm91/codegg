# TUI Editor & Files Panel Helix UX Improvement M001 — Closure Status

Status: closed

Source implementation plan: none. This roadmap's milestones were implemented
from the roadmap's own deliverable boundaries and exit conditions, so the
roadmap **is** the plan and the closure record below is the only evidence
artefact it has.

Source subsystem roadmap:

- `plans/subsystems/tui-editor-helix-ux-improvement-roadmap.md` (M001)

Repository baseline reviewed: `a588a264` (branch `feat/auto-20261020-513a8ce6`)

Applicable ADRs (unchanged by this milestone): none. This milestone adds no
protocol, storage, migration, authorization, or dependency change, and it
reopens no closed milestone of `tui-editor-helix-parity-roadmap.md`.

## 1. What landed

`src/tui/text_geometry.rs` now classifies a byte three ways — word
content, punctuation, whitespace — through `WordClass` and `classify_byte`.
`is_word_byte` is defined **in terms of** that classifier, so the two-way
predicate is no longer a second, parallel rule that can drift from the
three-way one. The run primitives (`WordRun`, `word_run_at`, `next_word_run`,
`prev_word_run`, `word_end_run`, `word_bounds`) are the vocabulary the
motion layer and the auto-selection share.

The multi-byte rule is unchanged and still stated by the classifier: every
byte `>= 0x80` is word content, so a UTF-8 character is never split and a
byte-at-a-time walk cannot mistake a character's interior for a boundary.

The interactive terminal's whitespace-token rule stays a **separate** rule in
the same module, visibly separate: it is a shell token, not a word, and
conflating the two would change what a shell command's argument means.

## 2. Exit conditions

| Exit condition | Result | Evidence |
|---|---|---|
| `classify_byte` is the single classifier | met | `src/tui/text_geometry.rs` — `classify_byte` is the only place a byte is classified |
| `is_word_byte` is defined in terms of it | met | `is_word_byte` delegates to `classify_byte(..) == WordClass::Word` |
| the multi-byte rule still holds | met | `a_multi_byte_character_is_a_word_and_not_a_separator`, `a_multibyte_character_is_never_split`, `a_string_and_a_snapshot_agree_on_word_boundaries_in_non_ascii_text` |
| punctuation runs, underscores, and multi-byte characters are covered | met | `an_underscore_is_word_content_and_a_punctuation_run_is_a_word_of_its_own`, `a_punctuation_run_is_one_word`, `the_terminal_panels_token_rule_is_a_different_rule_on_purpose` |

## 3. Residual risk and open items

**The text object's word is not the motion's word.** The `w` **text
object** selects word content only, while the `w` **motion** treats a
punctuation run as a word in its own right (Helix's category model). That
split was discovered while closing this milestone's successor: reusing the
motion's `word_end` for the object made `m i w` on `a.b` select `a.`. The
object now has its own run primitives (`word_content_run_at`,
`next_word_content_run`) and the motion keeps the three-way rule. The two
are documented against each other in `src/tui/text_geometry.rs` so the next
reader does not reunite them by accident.
