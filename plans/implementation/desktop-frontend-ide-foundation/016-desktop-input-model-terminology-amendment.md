# Desktop Frontend and IDE Foundation Milestone 016 — The Desktop Input Model's Terminology Amendment

Status: ready for handoff

Repository baseline: `dac94b3ad` — the M007-C closure commit on
`feat/desktop-keyboard-first-m007-c`, carrying the desktop's keymap, focus
model and bindings surface.

Source roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#milestone-007--desktop-gui-parity-line`
- M007 decomposition table: `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#m007-decomposition`

Planning basis:

- M007-C's plan reserved this step for the owner, and did not perform it:
  `plans/implementation/desktop-frontend-ide-foundation/013-desktop-keyboard-first-input.md`
  §12 — "Terminology amendment — a separate, owner-directed step. … The
  implementer **does not perform it** and does not treat its absence as a
  blocker." The milestone recorded the proposed vocabulary in its closure
  record at `plans/closure/desktop-frontend-ide-foundation/013-status.md` §13
  and left `plans/001-terminology-and-domain-model.md` untouched.
- **The owner directed the amendment on 2026-10-25**, before M007-D and
  M007-E land, so that the two later milestones inherit the vocabulary
  instead of each inventing a local meaning for the same words.
- `plans/002-long-term-roadmap.md:899` — "When implementation reveals that a
  term or ownership boundary is wrong, update the terminology document and
  long-term specification first, then adjust the roadmap." This plan follows
  that order, and the order is not optional: an amendment that landed in the
  roadmap first would leave the normative document contradicting it.

Long-term requirements:

- `plans/001-terminology-and-domain-model.md:5` — terminology is normative for
  plans, protocol types, schemas, architecture documents, tests and **UI
  labels**. A keymap is a UI-label surface, so its labels are in scope.
- `plans/000-long-term-specification.md:564` — "Exact keys remain
  configurable. The conceptual hierarchy and read-only observation behavior
  are normative." The concepts are normative and the bindings are not; the
  amendment records the vocabulary, never a key.
- `plans/000-long-term-specification.md:97` §4.4 — frontends render
  projections and must not own durable state. An input model is frontend-local
  presentation state, which this clause extends to name.
- `plans/002-long-term-roadmap.md:237` — "keyboard and focus-state regression
  tests" is a required test category. The amended vocabulary names the
  concepts that category is written against.

Applicable ADRs and architecture:

- `plans/adrs/ADR-0052-cross-frontend-command-execution-ownership.md`
  (accepted 2026-10-24) — controlling for the one distinction this amendment
  must not blur. Its `### Command` entry defines a **daemon-owned catalog
  entry**; the desktop's **action** is a renderer-local binding. Using one
  word for both is the incompatible local meaning
  `plans/002-long-term-roadmap.md:899` warns about.
- `plans/adrs/ADR-0010-desktop-frontend-and-shared-client-boundary.md`
  (accepted) — the desktop renderer holds presentation state and no daemon
  authority, which is exactly what an input model is.
- `architecture/desktop.md` §"The desktop's input model" — the document that
  describes the feature the vocabulary names.
- `architecture/tui.md` — the TUI's keymap is unchanged by this amendment.

Predecessor closure:

- **M007-C** — `plans/closure/desktop-frontend-ide-foundation/013-status.md`.
  The vocabulary this plan canonicalizes is the vocabulary M007-C ships.

Primary class: documentation

---

## 1. Objective

Give the desktop's input model the vocabulary the canonical documents
require, without changing a single line of behaviour.

The desktop now has a **keymap**, **chords**, **actions** and **binding
scopes**. None of the four words existed in the canonical documents before
M007-C, and each is a word this codebase already uses elsewhere with a
different meaning. Leaving them undefined is how a frontend ends up with its
own local meaning for a term the specification already owns.

The load-bearing distinction is **action** versus **command**:

- a **command** is an entry in the daemon-owned catalog, addressed by its
  identity, and dispatched by the daemon or by a frontend's bounded declared
  frontend-effect set;
- an **action** is a renderer-local binding — a named operation the frontend
  performs, with chords and a mouse affordance.

M007-D is implementing the command side and M007-E is implementing an editor
surface with its own key bindings. Both land after this plan, and both would
otherwise use the word "action" for something the ADR already calls a
command.

## 2. What the amendment changes, and what it must not

### 2.1 In scope

1. `plans/001-terminology-and-domain-model.md` — a new §16 with the four
   terms and the UI-label rule, plus §15's "action" bullet (§3.1).
2. `plans/000-long-term-specification.md` — the frontend-ownership clause
   (§3.2).
3. `plans/002-long-term-roadmap.md` — the Phase-5 deliverable that carries
   the vocabulary (§3.3).
4. `plans/registry.md` — this plan's registration and disposition (§6).
5. A closure record at
   `plans/closure/desktop-frontend-ide-foundation/016-status.md`.

### 2.2 Out of scope — explicit non-goals

- **No behaviour change.** No code, no test, no protocol, no storage, no
  migration, no DTO, no bridge target.
- **No TUI change.** The TUI's keymap, focus model and dispatch are unchanged.
  The TUI's own keymap has its own vocabulary, and this amendment does not
  impose the desktop's on it.
- **No renumbering of the canonical documents.** The terminology document's
  existing sections MUST keep their numbers. `plans/adrs/ADR-0052-…:260`
  refers to "the §15 `"command"` bullet", and that ADR is **accepted** and
  immutable — a renumbering would silently break an immutable document's
  cross-reference. The new entries therefore go inside an existing section.
- **No shift of any cited line.** The same reason, and the reason the new
  section is placed where it is. The terminology document's first **779** lines
  already carry citations from the accepted ADR-0052 (`:452-459`, `:702`,
  `:779`), from plan 012 (`:143`, `:672-676`) and from plan 014 (`:452-459`,
  `:759`), and from the registry. The amendment therefore adds its section
  **after** line 779 and renumbers the Review checklist from §16 to §17 —
  **§16 is cited nowhere**, so that renumbering breaks nothing. Every line up
  to and including 779 stays byte-identical to the baseline, which is checked
  mechanically rather than asserted (§4).
- **No new canonical term for "scope"** as a bare word. Scope is already
  provider-connection scope and authorization scope, and §1's fourth naming
  rule forbids using one term as interchangeable shorthand across those
  boundaries. The term is **binding scope**.

## 3. Required changes

### 3.1 Terminology (`plans/001-terminology-and-domain-model.md`)

Four entries in a **new §16 "Frontend input terms"**, placed after §15 and
before the Review checklist, which becomes §17:

| Entry | One-line sense |
|---|---|
| `### Keymap` | a frontend's own mapping from chords to actions; frontend-local presentation state, never daemon state |
| `### Chord` | a normalized key combination a frontend resolves against a keymap; presentation, not identity and not authorization |
| `### Action` | a named frontend operation, bound to chords and to a mouse affordance; **not** a `Command`, a `Run`, a `Job`, a shell command or a tool call |
| `### Binding scope` | the context a chord resolves in — global, region or overlay — which decides its owner |

Each entry MUST state what the term is **not**, in the document's own idiom,
and each MUST name the boundary it sits on. `### Action` in particular MUST
say that a frontend implementing an action by dispatching a command MUST NOT
hold a second catalog of its own.

Two further changes in the same document:

- `### Action` gains a **label** rule. A label is presentation, not a
  canonical term: it MUST NOT misname the concept it performs, it MAY use a
  shortened form where the context makes the referent unambiguous (exactly as
  §15 already permits the TUI to label work orders "Tasks"), and two frontends
  MAY display different labels for the same action or the same command
  identity. The desktop's current labels — "Toggle side panel", "Open project
  switcher", "Toggle tasks panel", "Keyboard help", "Key bindings" — are
  recorded as presentation labels so a reader does not mistake them for
  canonical terms.
- §15 "Prohibited ambiguous usage" gains an **"action"** bullet, immediately
  after the existing "command" bullet, because the two are the pair most
  likely to be confused and the bullet list is where the document already
  forbids exactly this kind of drift.

### 3.2 Specification (`plans/000-long-term-specification.md`)

§4.4 "Frontends render projections" gains one clause, after the existing
desktop-client paragraph: each frontend owns its own input model; a keymap,
its chord bindings, its focus model and its action set are frontend-local
presentation state and are not durable session, provider or scheduling
state; two frontends MAY bind different keys to the same operation; a
frontend's action is not a command; and a frontend that implements an action
by dispatching a command MUST NOT hold a second catalog of its own.

### 3.3 Roadmap (`plans/002-long-term-roadmap.md`)

Phase 5's deliverables gain one bullet recording that each frontend consumer
has its own input model as frontend-local presentation state, that the
vocabulary is canonicalized in the terminology document, and that a frontend's
action stays distinct from a daemon-owned `Command`.

Phase 5's exit criteria are **not** changed: this amendment adds vocabulary,
not behaviour, and no milestone's completion depends on it.

## 4. Verification

This milestone changes documentation only, so the verification is the
documentation guards and a citation check:

| Command | What it proves |
|---|---|
| `python3 scripts/check_adr_integrity.py` | the accepted ADR's §15 reference still resolves |
| `python3 scripts/check_comment_budget.py` | the added prose is inside the comment budget |
| `python3 scripts/check_work_plan_repository_binding.py` | this plan is bound to the repository it describes |
| `python3 scripts/check_adr_integrity.py` | no ADR was amended |
| a scripted pass over every `path:line` this plan and its closure record cite | every citation is in range |

**No cargo build, no test run and no desktop build** are required: no code
changes. `scripts/verify.sh quick` MAY be run for the guards it runs over the
planning documents, and its outcome MUST be reported either way.

## 5. Acceptance criteria

1. The four terms are defined in `plans/001-terminology-and-domain-model.md`
   with the boundaries each sits on, and the document's section numbers are
   unchanged.
2. The `### Command` entry and the new `### Action` entry are
   distinguishable by their text alone, and the prohibited-usage list carries
   an "action" bullet beside its "command" bullet.
3. The specification's §4.4 clause and the roadmap's Phase-5 bullet both
   name the frontend-local ownership of an input model, and the roadmap
   change follows the terminology and specification changes.
   The terminology document's lines 1–779 are byte-identical to the baseline,
   and the only section renumbering is §16 → §17, which nothing cites.
4. No behaviour, protocol, storage, DTO or code change lands.
5. No accepted ADR is edited.
6. Every `path:line` cited is in range.

## 6. Closure evidence required

1. The four entries, quoted as they landed.
2. The specification clause and the roadmap bullet, quoted as they landed.
3. The section-number check: the terminology document's section numbers
   before and after, with the ADR-0052 §15 reference confirmed still
   correct.
4. The verification outcomes from §4.
5. A statement that no code changed, with `git diff --stat` over the
   non-planning trees showing empty.
