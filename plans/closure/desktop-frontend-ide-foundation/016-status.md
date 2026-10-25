# Desktop Frontend and IDE Foundation Milestone 016 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/desktop-frontend-ide-foundation/016-desktop-input-model-terminology-amendment.md`

Source subsystem roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md`

Applicable ADR (controlling):

- `plans/adrs/ADR-0052-cross-frontend-command-execution-ownership.md` — accepted
  2026-10-24. It is the reason this amendment exists: its `### Command` entry
  defines a **daemon-owned catalog entry**, and the desktop's **action** is a
  renderer-local binding. One word for both is the incompatible local meaning
  `plans/002-long-term-roadmap.md:899` warns about.

Repository baseline reviewed: `dac94b3ad` — the M007-C closure commit on
`feat/desktop-keyboard-first-m007-c`.

Implementation commit on this milestone's branch:

- the amendment itself, in the same change as this record.

## 1. Executive finding

The desktop shipped a keymap, chords, actions and binding scopes without any
of those four words existing in the canonical documents. M007-C was right to
record the vocabulary and leave the normative document alone — the amendment
is a normative change and therefore the owner's step. **The owner directed it
on 2026-10-25**, before M007-D and M007-E land, so the two later milestones
inherit one vocabulary instead of each inventing a local meaning for the same
words.

The amendment changes **no behaviour**. It adds four terms, one label rule,
one specification clause and one roadmap bullet.

## 2. Requirement-to-evidence matrix

| Plan criterion | Evidence | Verdict |
|---|---|---|
| §5.1 — the four terms are defined, with the boundaries each sits on, and no cited line shifts | §3.1 — `### Keymap` `:800`, `### Chord` `:808`, `### Action` `:814`, `### Binding scope` `:822`; the document's lines 1–779 are byte-identical to the baseline's, and the only renumbering is §16 → §17, which nothing cites | met |
| §5.2 — the `### Command` and `### Action` entries are distinguishable by their text alone, and the prohibited-usage list carries an "action" bullet beside its "command" bullet | §3.2 — §15's `"command"` bullet and the new `"action"` bullet at `:812`; the two entries' "is not" clauses | met |
| §5.3 — the specification's §4.4 clause and the roadmap's Phase-5 bullet both name the frontend-local ownership of an input model, and the roadmap change follows the terminology and specification changes | §3.3 | met |
| §5.4 — no behaviour, protocol, storage, DTO or code change lands | §4 — `git diff --stat` over the non-planning trees is empty | met |
| §5.5 — no accepted ADR is edited | §4 — `git diff --stat -- plans/adrs/` is empty; `plans/adrs/ADR-0052-…:260`'s "§15" reference still resolves | met |
| §5.6 — every `path:line` cited is in range | §4 | met |

## 3. What landed

### 3.1 The terminology document

A **new §16 "Frontend input terms"**, placed after §15 and before the Review
checklist, which becomes §17:

**`### Keymap`** (`:800`) — "A frontend's own mapping from chords to actions."
It states that a keymap is **frontend-local presentation state** — not
durable session state, not provider state, not shared with the daemon — that
each frontend owns its own, that two frontends MAY bind different keys to the
same operation, that the bindings are the user's, and that a keymap is not a
`Command` catalog.

**`### Chord`** (`:808`) — "A key combination, normalized into a fixed modifier
order and a lowercased key token, that a frontend resolves against a keymap."
It states that a chord is a presentation-layer input, that it is not an
identity and not an authorization subject, that the same operation MAY be
reachable by more than one chord, and that a chord MAY be rebound without
changing what the operation does.

**`### Action`** (`:814`) — "A named operation a frontend can perform, bound
to zero or more chords and to a mouse affordance." It states that an action is
**frontend-local**, that it is not a `Command`, a `Run`, a scheduler `Job`, a
shell command or a tool call, and that a frontend implementing an action by
dispatching a `Command` MUST NOT thereby become a second command catalog —
the commands a frontend can dispatch are the bounded, declared frontend-effect
set of the daemon's catalog.

**`### Binding scope`** (`:822`) — "The context a chord is resolved in, which
decides which action owns it." It names the three scopes — **global**,
**region**, **overlay** — and states that the scope, rather than a mode,
decides the owner, so two scopes MAY bind the same chord to different actions
without either being ambiguous.

**Why "binding scope" and not "scope".** The bare word is already
provider-connection scope and authorization scope, and §1's fourth naming rule
forbids using one term as interchangeable shorthand across those boundaries.
The entry says so explicitly, so the next reader does not have to rediscover
it.

**The label rule**, in the `### Action` entry. A label is presentation, not a
canonical term: it MUST NOT misname the concept it performs; it MAY use a
shortened form where the context makes the referent unambiguous, exactly as
§15 already permits the TUI to label work orders "Tasks"; and two frontends
MAY display different labels for the same action or the same command
identity. The desktop's current labels — "Toggle side panel", "Open project
switcher", "Toggle tasks panel", "Keyboard help" and "Key bindings" — are
recorded as presentation labels under the rule, so a reader does not mistake
them for canonical terms.

### 3.2 The prohibited-usage list

§15 "Prohibited ambiguous usage" gains an `"action"` bullet immediately after
the existing `"command"` bullet (`:782`), which stays at `:779`:

> "action" when the intended object is a `Command`, a `Run`, a scheduler job,
> a shell command, or a tool call. An `Action` is a frontend-local binding; a
> `Command` is a catalog entry addressed by its identity, and a frontend that
> implements an action by dispatching a command MUST NOT hold a second
> catalog of its own;

The bullet sits beside the "command" bullet because the two are the pair most
likely to be confused, and that list is where the document already forbids
exactly this kind of drift.

### 3.3 The specification and the roadmap

`plans/000-long-term-specification.md` §4.4 "Frontends render projections"
gains one clause after the existing desktop-client paragraph: each frontend
owns its own input model; a keymap, its chord bindings, its focus model and
its action set are frontend-local presentation state and are not durable
session, provider or scheduling state; two frontends MAY bind different keys
to the same operation; a frontend's action is not a command; and a frontend
that implements an action by dispatching a command MUST NOT hold a second
catalog of its own.

`plans/002-long-term-roadmap.md` Phase 5's deliverables gain one bullet:
each frontend consumer has its own input model — keymap, chords, focus model
and actions — as frontend-local presentation state, with the vocabulary
canonicalized in the terminology document and a frontend's action kept
distinct from a daemon-owned `Command`. Phase 5's **exit criteria are
unchanged**: this amendment adds vocabulary, not behaviour.

**The order was the one `plans/002-long-term-roadmap.md:899` requires** —
terminology first, then the long-term specification, then the roadmap. An
amendment that landed in the roadmap first would leave the normative document
contradicting it.

## 4. Verification executed

| Command | Outcome |
|---|---|
| `python3 scripts/check_adr_integrity.py` | **pass** — "ADR integrity fixtures passed; active ADR IDs are unique and local paths resolve" |
| `python3 scripts/check_comment_budget.py` | **pass** — "comment budget guard passed" |
| `python3 scripts/check_work_plan_repository_binding.py` | **pass** — "repository-binding ownership check passed" |
| a line-by-line comparison of the terminology document's **first 779 lines** against the baseline's | **byte-identical** — so no citation anywhere shifts |
| `git diff --stat -- plans/adrs/` | **empty** — no accepted ADR edited |
| `git diff --stat` over the non-planning trees | **empty** — no code, test, protocol, storage, DTO or bridge change |
| `scripts/verify.sh quick` | **pass** — "Quick verification passed." |

**No cargo build, no test run and no desktop build.** The milestone changes
documentation only; the plan's §4 says so, and running them would have
produced evidence of nothing this change touched.

### The renumbering check, stated plainly

The terminology document's first **779** lines are byte-identical to the
baseline's, and that is a **requirement**, not an accident.

Those 779 lines carry live citations. `plans/adrs/ADR-0052-cross-frontend-command-execution-ownership.md`
cites `:452-459` in its **Context**, in its **Decision drivers** and in
**Decision item 4**, and cites `:702` and `:779` in Decision item 7's landing
note; plan 012 cites `:143` and `:672-676`; plan 014 cites `:452-459` and
`:759`; and the registry's source-verification entry for ADR-0052's landing
cites `:702` and `:779`. ADR-0052 is **accepted** and immutable, so its
decision text must not be rewritten to re-anchor those pointers — and a
pointer that no longer points at its target is a broken cross-reference
whether or not a guard checks it.

The amendment therefore places its section **after** line 779 and renumbers
the Review checklist from §16 to §17. **§16 is cited nowhere** — the three
places that cite sections of this document cite `§§5, 7–8`, `§§7, 12–13` and
"the §15 `"command"` bullet", and none cites §16 — so that renumbering breaks
nothing either. The check is mechanical: a line-by-line comparison of the
document's first 779 lines against the baseline's, reported in §4.

## 5. Invariant review

- **No behaviour changed.** The milestone's `git diff --stat` over the
  non-planning trees is empty.
- **No accepted ADR changed.** ADR-0052 is accepted and immutable; its text
  is untouched, and its cross-reference into the terminology document still
  resolves.
- **The TUI is untouched.** `architecture/tui.md` is not edited, and the
  amendment adds no term the TUI's own keymap is measured against.
- **The desktop's behaviour is untouched.** The keymap, the chord machinery,
  the focus model and the bindings surface are exactly as M007-C shipped them;
  only their *names* are now canonical.
- **No section renumbered.** Checked by diffing the `## ` headings against the
  baseline's, not by counting.

## 6. Failure and recovery review

- **A reader who searches for "scope" must find "binding scope".** The
  `### Binding scope` entry says why the bare word was refused, so the
  decision is legible rather than merely applied.
- **A reader who confuses "action" with "command" must be stopped.** The
  `### Action` entry names what an action is not, §15's new bullet forbids the
  ambiguous usage in new design documents, and the specification clause says a
  frontend MUST NOT hold a second catalog.
- **A frontend that wants a different word must not need a new amendment.**
  The label rule permits per-frontend labels, so the vocabulary constrains
  the concepts and leaves the strings to the frontend.

## 7. Migration and compatibility review

None required. No identifier, schema, protocol field, storage layout or
public interface changed. The amendment adds vocabulary to three normative
documents and nothing else.

## 8. Security review

Not applicable: no code, no authority, no capability, no Tauri permission and
no daemon surface is touched. The amendment records that a frontend's input
model is frontend-local presentation state and is not shared with the daemon —
a statement that *narrows* what a frontend may own, rather than widening it.

## 9. Documentation and operations

| Document | What changed |
|---|---|
| `plans/001-terminology-and-domain-model.md` | four entries in §6, the label rule, and §15's `"action"` bullet |
| `plans/000-long-term-specification.md` | §4.4's frontend-ownership clause |
| `plans/002-long-term-roadmap.md` | Phase 5's input-model deliverable |
| `plans/registry.md` | this milestone's row and disposition |
| `plans/implementation/desktop-frontend-ide-foundation/016-desktop-input-model-terminology-amendment.md` | the plan |
| this record | the closure evidence |

## 10. Unresolved findings

| Finding | Severity | Owner |
|---|---|---|
| The canonical documents still have **no** term for *overlay* or *sidebar*. M007-C's plan §5.3 named those as descriptive only. The amendment deliberately did not add them: the desktop's regions are a presentation concept with no canonical counterpart, and inventing one now would be a second speculative term. | low | accepted; a future frontend milestone may propose them with the same discipline |
| The **UI-label rule** constrains labels but does not enumerate them per frontend, so a new frontend MAY choose labels the rule permits. That is deliberate: the document constrains concepts, not strings. | low | accepted |
| M007-E's editor surface currently declares its chords in its own table (`apps/desktop/src/editor/keys.ts`) rather than in M007-C's keymap, because M007-C's files were not in M007-E's worktree when it was built. The vocabulary this milestone canonicalizes makes that a **second dispatch table for the same concepts**, which the terminology now names — and M007-E must fold its chords into the keymap before it closes. | medium | M007-E's milestone, before its closure |

**No medium or high finding keeps this milestone open.** The one medium
finding belongs to M007-E and is recorded here so it is not lost.

## 11. Roadmap disposition

`plans/subsystems/desktop-frontend-ide-foundation-roadmap.md`:

- Phase 5's deliverables gain the input-model bullet (§3.3). **The M007
  decomposition table, the M007 rows and the line's status row are
  unchanged** — this milestone is not one of M007-A…M007-E and does not
  change the line's shape.

## 12. Registry updates

`plans/registry.md`:

- a row for this milestone in `## Dependency-ready implementation plans`,
  pointing at this closure record;
- the `## Active subsystem roadmaps` desktop row records that the canonical
  documents now name the desktop's input model;
- a dated entry in `## Source verification`.

## 13. Handoff notes

- **M007-D** should use *command* for the daemon-owned catalog entry and
  *action* for a frontend-local binding, and MUST NOT describe its catalog as
  a set of "actions". Its `### Command` entry is already canonical.
- **M007-E** must fold `apps/desktop/src/editor/keys.ts` into
  `apps/desktop/src/keymap.ts`'s declaration table, under the editor's
  scope, and delete its own resolver — the terminology now names two
  different concepts that its two tables currently blur.
- **Neither** may add a canonical term for *scope* on its own: the term is
  *binding scope*, and §15 now forbids the ambiguous usage.
