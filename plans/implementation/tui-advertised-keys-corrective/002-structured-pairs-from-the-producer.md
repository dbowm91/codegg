# TUI Advertised Keys Corrective Milestone 002 — I4 consumes structured `(key, verb)` pairs

Status: conditionally closed

Repository baseline: `7642b9e38` on `feat-auto-20261109-f8abad0d`

**The tree this plan was written against is uncommitted.** HEAD is `7642b9e38`;
the working tree carries **235 changed or untracked paths** (measured,
`git status --porcelain | wc -l`). `scripts/verify.sh full` was green on that tree
at **2026-11-12** — 17,326 workspace tests and 8,400 feature-pass tests, 0
failures, render corpus 1227 / 0 (**parent-run**, not repeated by this session).
Every number below is a property of that tree, not of a SHA.

Source closure record:

- `plans/closure/tui-modularity-and-render-invariants/006-status.md` §1 and §10 —
  the finding: "The I4 check parses rendered strings instead of the structured
  `(key, verb)` pairs the plan says `HintLine` supplies … Decide deliberately:
  either convert the check to structured pairs, or amend the plan to record that
  string parsing was accepted and why. Do not leave the plan and the code
  disagreeing."

Source implementation plan:

- `plans/implementation/tui-modularity-and-render-invariants/006-advertised-keys-are-handled.md`
  §2 ("I4 needs the advertised verbs as structured `(key, verb)` pairs, and
  parsing them back out of a rendered string is exactly the fragility this
  repository avoids. `HintLine` supplies the pairs") and work package A.

Source roadmap:

- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md` §2 (I4) and
  `architecture/tui.md` §"One hint vocabulary" (the statement that the baseline
  is keyed by **spelling**).

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior` — through I4, the
  advertised interaction concepts are the delivered ones.
- `plans/003-planning-process.md` — I4 is an interim invariant; this milestone
  changes its *evidence*, not its meaning.

Applicable ADRs:

- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` — a hint describes
  a key; it never decides which stage claims a chord. The structured form must
  not become a second claim table.
- `plans/adrs/ADR-0052-cross-frontend-command-execution-ownership.md` — no
  catalog change.

Primary class: **invariant**. The rendered frame and every surface's behaviour
must be unchanged; what changes is how the invariant obtains its evidence.

## 1. Objective

Make `tests/tui_render/invariants/baseline.rs` obtain each surface's advertised
`(key, verb)` items from the producer — `HintLine` — instead of parsing them back
out of a rendered string, **without moving a single recorded row**.

The milestone also closes the measurement hole that the string parser creates: a
surface can currently advertise a key the parser cannot read (`Home`, `End`,
`PageUp`, `PageDown`, `⌫`, `[`, `]`) and the invariant silently skips it. After
this milestone an unreadable advertisement is a **finding**, not a skip.

## 2. Why this milestone is ready

The structure already exists and is already built by every hint under the one
vocabulary: `HintLine` holds `items: Vec<HintItem>` with `key`, `verb` and
`required` (`src/tui/components/hint_line.rs:10-21`), and every hint function
builds one. Only two things are missing — the items are private, and
`Component::keymap_hint()` renders them to a `String` at the trait boundary
(`component.rs:241`, 14 overrides). Both are mechanical.

It is ordered after milestone 001 because 001 must first establish that the
recorded set is state-correct and inspectable; converting the *source* of the set
before that would change two variables at once.

## 3. Current implementation evidence

### 3.1 `HintLine` already holds the pairs — privately

`hint_line.rs:10-21`: `struct HintItem { key: String, verb: String, required:
bool }`, private, inside `pub struct HintLine { items: Vec<HintItem> }`. `push`
lowercases only the verb (`:30-37`); `render(budget)` orders required items first
and joins with `HINT_SEPARATOR` (`:49-59`). There is **no accessor**. Everything
downstream re-parses the rendered string, which is why `architecture/tui.md`
records that "the I4 baseline is keyed by **spelling**, so re-spelling an
advertised key re-banks that ratchet rather than fixing a layout".

### 3.2 Two producers, not one

The plan's §2 assumed `HintLine` supplies every advertised pair. It supplies
only the `keymap_hint()` half. Of the **18** surfaces in the baseline, **8** have
a `keymap_hint` (all `HintLine`-backed): `Diff`, `Files`, `Keybind`, `Mcp`,
`ResearchBrowser`, `Review`, `SecurityReview`, `Tree`. The other **10** —
`Agent`, `Connect`, `Help`, `Model`, `PanelManager`, `Permission`, `ProjectChat`,
`Session`, `Template`, `WorkspaceDashboard` — advertise **only** from a painted
footer, and those rows are hand-formatted string literals:

- **26 dialog modules** paint a `|`-separated advertisement row: `agent`,
  `confirm`, `connect`, `diff`, `goto`, `help`, `import`, `info`, `keybind`,
  `mcp`, `model`, `permission`, `plan`, `plugin`, `policy_selector`,
  `project_picker`, `question`, `research`, `review`, `security_review`,
  `session`, `share`, `source_preview`, `template`, `theme`, `tree` (measured by
  enumerating the string literals in `src/tui/components/dialogs/*.rs`).
- Of the 10 footer-only baseline surfaces, the I4-relevant render sites are
  **18**: `Agent` 1 (`agent.rs:214`), `Connect` 2 (`:921`, `:1436`), `Help` 4
  (`:589`, `:591`, `:593`, `:596`), `Model` 1 (`:1278`), `PanelManager` 2
  (`:359`, `:360`), `Permission` 2 (`:315`, `:317`), `ProjectChat` 1
  (`chat_browser::normal_hints`, `:818-823`), `Session` 2 (`:1306`, `:1317`),
  `Template` 2 (`:206`, `:354`), `WorkspaceDashboard` 1 (`:317`).
- `WorkspaceDashboard` is the sharpest case: its row already uses the one
  separator (`·`) but is a raw `Line::from(&str)`, so it is **outside**
  `HintLine` and outside the vocabulary scan's reach, which reads
  `keymap_hint()` only (`tests/tui_render/invariants/vocabulary.rs:57-67`).

So the conversion is **not** "expose `HintLine`'s items". It is that plus
converting 18 footer render sites, which is why it is its own milestone.

### 3.3 The parser's blind spot, measured

`key_of` (`baseline.rs:249-292`) understands `Esc`/`Enter`/`Tab`/`space`, the
four arrows, `Shift+X`, `⇧X`, `^X` and single alphanumerics. It returns `None`
for every other spelling, and `spellings` reads only the **first** whitespace
token of a part (`:296-305`). Advertised keys that are therefore never pressed:

| Spelling | Advertised at | Consequence |
|---|---|---|
| `Home`, `End` | `diff.rs:34`, `task_view.rs:47` | two advertised verbs unmeasured |
| `PageUp`, `PageDown` | `ui_node.rs` hint; `Diff`/`Connect`/`SecurityReview` footers | paging unmeasured where the footer spells it |
| `⌫` | `research.rs` hint | backspace unmeasured |
| `[` | `queue.rs:64` (`push("[ ]", "reorder")`) | the first token is `[`, unreadable — **both** keys of the pair are lost |
| `]`, `[` | `security_review.rs:47` (`]/[`) | hunk navigation unmeasured |
| `F5`-style function keys | none today | rejected by the guard if banked |

The guard mirrors the parser (`NAMED_KEYS`, `ARROW_KEYS`, `CHORD_RE`, `BARE_RE`
in `scripts/check_tui_render_invariant_baseline.py:87-90`) and rejects a recorded
spelling the parser cannot read, so these keys cannot even be banked. **The 61
entries measure a subset of the advertised set and there is no record of what the
subset omits.**

### 3.4 The 32-surface vocabulary scan is the constraint

`tests/tui_render/invariants/vocabulary.rs` renders every dialog fixture's
`keymap_hint()` plus four panel/composer rows and asserts a floor of **32**
surfaces (`:144-151`) and an exact recorded table (`:105-134`). Any change to
`keymap_hint()`'s return type or text moves that table. The conversion must keep
both the floor and the strings.

## 4. Invariants that must not regress

- **The recorded set does not move.** `[advertised_keys]` after this milestone
  contains the **same rows** as before it — same surfaces, same key spellings, in
  the same order the harness prints. Captured before and after and compared with
  `diff`; an empty diff is the criterion.
- **`[contrast]` is untouched at 2,495 rows.**
- **The render corpus stays green** (1227 / 0) and the invariant module stays at
  its post-001 count plus this milestone's new tests.
- **The one vocabulary holds.** The vocabulary scan's floor stays ≥ 32, its
  recorded table is unchanged except for any row milestone 001 already changed,
  and `HintLine` remains the only producer.
- **A hint never becomes a claim table.** The structured items describe a key;
  they do not say which stage owns it (ADR-0042). No `handles_key()`-shaped
  method is added.
- **No key changes owner; no command catalog change.**
- **The render-cost shape guard and the comment ratchet** are unchanged.

## 5. Scope

### In scope

- `HintLine`: expose its items (a public item type and an iterator), keeping
  `push`, `push_required` and `render` behaviour byte-identical.
- `Component`: make `keymap_hint()` a provided method over a new
  `keymap_hint_line() -> Option<HintLine>`, and add
  `advertised_hints() -> Vec<HintLine>` for the lines a surface paints.
- Convert the 8 `keymap_hint` implementations and the 18 footer sites of the 10
  footer-only baseline surfaces.
- `Section::hint` (4 production call sites: `plan.rs:115`, `plan.rs:196`,
  `plan.rs:297`, `question.rs:300`) gains a `HintLine`-taking form; its string
  form stays for the panels.
- The harness: press each item's key from the structured set; keep the rendered
  row scan as a **completeness cross-check** that fails when a painted part is
  not in the structured set.
- The parser: learn the named keys in §3.3, and report an unreadable advertised
  spelling as a finding rather than skipping it.
- The guard's mirror of the parser follows.

### Explicitly out of scope

- **The 16 other dialog modules with hand-formatted rows** (`confirm`, `goto`,
  `import`, `info`, `plan`, `plugin`, `policy_selector`, `project_picker`,
  `question`, `share`, `source_preview`, `theme`, `task_schedule`, `queue`,
  `task_view`, `ui_node`). They never enter the baseline; converting them is a
  vocabulary-coverage milestone, not this one, and the cross-check is therefore
  scoped to the 18 baseline surfaces.
- **The panel seam** (`PanelHints::composer_hints` and the four panel
  `composer_hints` implementations) keeps returning `Option<String>`: I4
  measures dialogs and the guard rejects a `panel:` advertisement outright
  (R4). The vocabulary scan already reads those rows.
- **Making the verb checkable against the arm.** Having the verb is a
  prerequisite, not the check; "the verb matches what the arm does" is a new
  invariant and is not invented here.
- **`[contrast]` re-banking, storage, protocol, runtime.**

## 6. Required production changes

### Core/domain, storage, migrations, protocol, DTOs, runtime, concurrency

None.

### Frontend or operator surface

- `src/tui/components/hint_line.rs` — `HintItem` becomes public with accessors
  (`key()`, `verb()`, `is_required()`), `HintLine::items()` returns an iterator,
  and `HintLine::from_items` is added for surfaces that build a row
  programmatically. No rendering change.
- `src/tui/components/component.rs` — add
  `fn keymap_hint_line(&self) -> Option<HintLine> { None }` and
  `fn advertised_hints(&self) -> Vec<HintLine>` (default: the keymap line);
  `fn keymap_hint(&self) -> Option<String>` becomes
  `self.keymap_hint_line().map(|line| line.render(0))`. **No call site changes** —
  that is the point of keeping the provided method.
- The 8 `keymap_hint` implementations return `Some(hint())` from
  `keymap_hint_line()` instead of `Some(hint().render(0))`.
- The 10 footer-only surfaces build their footer as a `HintLine` and return it
  from `advertised_hints()`; the 18 render sites paint that line (for the rows
  that sit on a `Section` edge, via `Section::hint_line`).
- `src/tui/components/section.rs` — `hint_line(HintLine)` stores the line and
  paints `line.render(0)`; `hint(&str)` delegates to it.
- **Behaviour is unchanged**: the rendered strings are byte-identical, which the
  vocabulary table and the render corpus prove.

### Security and authorization

None.

### Documentation and static guards

- `architecture/tui.md` — the one-vocabulary section records that the hint's
  items are the invariant's input, and that the rendered row is a cross-check.
- `scripts/check_tui_render_invariant_baseline.py` — the key-spelling mirror
  learns the named keys; a standing rule rejects an `advertised_keys` entry whose
  spelling the harness cannot press.
- `docs/tui-render-invariant-baseline.toml` — header only (no row change).

## 7. Ordered work packages

### Work package A — Expose the items

Required changes: §6's `hint_line.rs` additions.

Acceptance evidence: `cargo test --lib -- tui::components::hint_line` → 7 passed
plus one new test that `items()` yields the same `(key, verb)` sequence
`render(0)` joins, including the empty-verb case and the required-first order.

### Work package B — The trait seam

Required changes: `keymap_hint_line`, the provided `keymap_hint`, the 8
conversions.

Acceptance evidence: `cargo test --test tui_render -- vocabulary` → 3 passed,
with the recorded hint table **byte-identical** to its pre-milestone value. This
is the proof that rendering did not change.

### Work package C — The 18 footer sites

Required changes: build each footer from `HintLine`; return it from
`advertised_hints()`; `Section::hint_line`.

Acceptance evidence: the vocabulary scan still enumerates ≥ 32 surfaces; a new
test asserts that for each of the 10 footer-only surfaces the painted row and
`advertised_hints()` yield the same key set (the cross-check, in miniature).

### Work package D — The harness reads the structure

Required changes: `measure_advertised_keys` takes each surface's `Vec<HintLine>`,
presses every item's key (fresh fixture per key, from milestone 001), and treats
the key as the invariant's subject. The rendered-row scan stays and becomes a
cross-check: for each painted part, its key must appear in the structured set,
or the measurement reports a surface that advertises outside its own declaration.

Acceptance evidence: `[advertised_keys]` is byte-identical before and after
(`diff` of the section is empty); `cargo test --test tui_render -- invariants` →
the post-001 count, 0 failed.

### Work package E — The parser's blind spot

Required changes: `key_of` learns `Home`, `End`, `PageUp`, `PageDown`,
`Backspace`/`⌫`, `[`, `]`; `spellings` reads **every** token of a part for a
multi-key part (`[ ]` must yield both keys); an unreadable advertised spelling is
a reported finding rather than a `continue`.

Acceptance evidence: a new test presses each newly readable key against a fixture
that advertises it (`Diff`/`Home`, `TaskView`/`End`, `Queue`/`[` and `]`,
`SecurityReview`/`]`, `ResearchBrowser`/`⌫`); the guard's mirror accepts exactly
those spellings and rejects one it does not (`F5`).

### Work package F — Non-vacuity for the new path

Required changes: a fixture that advertises **only** through a painted footer
must still be measured (the 10 surfaces are the live proof; the fixture makes it
a test), and a fixture whose structured set omits a painted pair must fail the
cross-check.

Acceptance evidence: two new tests, one passing and one failing-fixture; each
names the property it proves.

## 8. Failure, cancellation, restart, and contention semantics

Not applicable: the harness is synchronous and per-surface; no durable state, no
process, no queue.

One hazard is stated rather than hidden: **the cross-check can fail on a surface
the milestone did not convert.** It is scoped to the 18 baseline surfaces for
exactly that reason; a global cross-check would fail on the other 16 modules and
push an implementer to convert them here, which §5 forbids.

## 9. Compatibility and migration

No storage, protocol or configuration migration. `Component::keymap_hint()`
keeps its signature and its output; every existing test that asserts a hint
string keeps passing unchanged. The new methods are additive, and a component
that implements neither new method behaves exactly as today (it advertises
nothing to the structured path, and the cross-check reports it only if it paints
an advertisement while being one of the 18).

The one deliberate asymmetry: a `keymap_hint()` implementation that forgets to
move to `keymap_hint_line()` would silently stop feeding the structured set while
still rendering. The cross-check on the 18 catches it, and the vocabulary table
catches a change in text; a `--self-test`-style guard for "every `keymap_hint`
override also overrides `keymap_hint_line`" is **not** added, because a source
regex over Rust is the anti-pattern this repository avoids — the structural check
is the cross-check plus the 8 surfaces' tests.

## 10. Required tests

### Focused unit tests

- `hint_line`: `items()` equals what `render(0)` joins; required items first;
  empty verb preserved.
- `hint_line`: `from_items` round-trips a built row.
- `component`: the provided `keymap_hint()` renders `keymap_hint_line()`.

### Integration tests

- The cross-check over the 18 surfaces: structured set ⊇ painted set.
- A footer-only surface (`WorkspaceDashboard`, which has no `keymap_hint`) is
  measured.
- The parser: each named key in §3.3 is pressed and, on a fixture where it acts,
  reads as handled.
- The recorded set is unchanged.

### Restart and recovery tests

None.

### Contention and cancellation tests

None.

### Security and negative tests

- A fixture that paints an advertisement outside its structured set fails the
  cross-check.
- An advertised spelling the parser cannot read is a finding, not a skip.
- The guard rejects a banked entry whose spelling the harness cannot press.

### Migration and compatibility tests

- `[advertised_keys]` captured before and after: `diff` empty.
- The vocabulary recorded table: unchanged.

## 11. Required verification commands

```bash
# The invariant module, after milestone 001's harness work.
cargo test --test tui_render -- invariants
# expect: test result: ok. <31 + this milestone's new tests> passed; 0 failed;
#         (1227 + new tests − that count) filtered out

# The one-vocabulary scan and its recorded table.
cargo test --test tui_render -- vocabulary
# expect: 3 passed; 0 failed; and the recorded table byte-identical

# The recorded dialog gaps.
cargo test --test tui_render -- dialog_dispatch_coverage
# expect: 7 passed; 0 failed

# The structured-pairs unit tests, by module path.
cargo test --lib -- tui::components::hint_line
# expect: 7 passed before WP A's additions; 9 after; 0 failed

# The ratchet guard, unchanged in count.
python3 scripts/check_tui_render_invariant_baseline.py
# expect: check_tui_render_invariant_baseline: 2 advertised-key, 2495 contrast
#         and 0 hint-truncation entries recorded; every one reproduces and none
#         was added unbanked   (exit 0)

python3 scripts/check_tui_render_invariant_baseline.py --self-test
# expect: self-test: <n> cases passed, no "... cases failed" line, exit 0

# Authority, catalog and render shape must be unchanged.
python3 scripts/check_tui_composer_key_authority.py
python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_command_catalog_ownership.py
python3 scripts/check_tui_render_cost.py

# Formatting, linting and the gates.
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
scripts/verify.sh quick
scripts/verify.sh full
```

Every filter above names a module or a full test id this session checked exists:
`invariants/` holds 31 test functions today, `vocabulary.rs` 3,
`dialog_dispatch_coverage.inc` 7, `hint_line.rs` 7. **Do not cite
`cargo test --test tui_render -- tui_render_invariants`** — no test has that id,
the filter selects zero, and a zero-test filter exits 0.

## 12. Documentation updates

- `architecture/tui.md` — the one-vocabulary section: the items are the
  invariant's input; the rendered row is a cross-check; a surface that paints an
  advertisement outside its declaration is a finding.
- `tests/tui_render/invariants/baseline.rs` module doc — the two layers (items,
  cross-check) and why both exist.
- `src/tui/components/component.rs` — the doc on `keymap_hint_line`,
  `keymap_hint` and `advertised_hints` says which is the authority.
- `src/tui/components/hint_line.rs` — the module doc stops saying the items are
  an implementation detail.
- Closure record at `plans/closure/tui-advertised-keys-corrective/002-status.md`.

## 13. Acceptance criteria

1. `[advertised_keys]` is byte-identical to its state at the start of this
   milestone. Checkable by capturing
   `sed -n '/^\[\[advertised_keys\]\]/,/^# \[\[contrast\]\]/p'` before and after
   and requiring an empty `diff`.
2. `cargo test --test tui_render -- invariants` passes with **0 failed**, and
   the i4 test's per-key report shows each key arriving from the structured set,
   not from a parsed row.
3. `cargo test --test tui_render -- vocabulary` → 3 passed with the recorded
   hint table identical.
4. `cargo test --lib -- tui::components::hint_line` → 9 passed (7 existing + 2
   new), 0 failed.
5. `WorkspaceDashboard`'s `Tab` is measured although the surface has no
   `keymap_hint`; a test asserts its advertised set comes from
   `advertised_hints()`.
6. A fixture that paints a pair outside its structured set fails the
   cross-check; an unreadable spelling in the structured set is a finding.
7. The named keys in §3.3 are pressable and the guard's mirror accepts them;
   `F5` is still rejected.
8. `cargo fmt --check`, `clippy -D warnings`, `scripts/verify.sh quick` and
   `scripts/verify.sh full` are green; the render corpus is 1227 / 0.

## 14. Stop conditions

Report rather than improvise when:

- **a converted footer changes a rendered byte.** The vocabulary table and the
  render corpus are the detectors; a change there means the conversion is not
  mechanical and must stop.
- **the recorded set moves.** Any row appearing or disappearing in this milestone
  is a defect of the conversion, not a repair: 001 already decided every row.
- **the footer site count exceeds the 18 enumerated in §5.** Then the conversion
  is not the bounded change this plan describes; stop and record the accepted
  string-parsing approach instead, with the reason (the record permits exactly
  that outcome, and a plan that promises work it has not bounded is a trap).
- **`Section::hint`'s string form must change for the panels.** It must not: the
  panel seam is out of scope and keeps its string.
- **a surface can only express its advertisement by parsing its own rendered
  row.** Then the structure is not where the plan says it is, and the conversion
  must report the surface instead of inventing a second representation.
- **`[contrast]` moves.** Stop; M007 owns it.

## 15. Closure evidence required

- The `[advertised_keys]` section before and after, with an empty `diff`.
- The vocabulary recorded table before and after, with an empty `diff`.
- The per-key report showing structured items as the source.
- The two non-vacuity tests (footer-only measured; out-of-declaration painted
  pair rejected).
- The parser table from §3.3, each row now pressable, and the guard's mirror
  updated with one rejection case.
- The guard and `--self-test` outputs; the authority, catalog and render-cost
  guards; `fmt`, `clippy`, `quick`, `full`.
- A closure record at `plans/closure/tui-advertised-keys-corrective/002-status.md`.

## 16. Handoff notes

The reason this is a separate milestone is not that the code is hard; it is that
the plan's sentence "`HintLine` supplies the pairs" is only true for 8 of the 18
surfaces. The other 10 never touch `HintLine` — they paint a string literal — so
a conversion that only exposes the items would leave the majority of the baseline
parsed exactly as today, and the plan, the code and the closure record would
disagree again in a new way.

Keep `keymap_hint()` returning `Option<String>`. Every panel, every test and the
vocabulary scan read it; changing that signature is the wide, risky version of a
change that has a narrow one.

The cross-check is load-bearing. Without it, a surface can stop declaring what it
paints and the structured path would quietly measure less than the screen shows —
which is the failure mode the rendered-string parser has today, wearing the
opposite hat.

Do not make the verb checkable against the arm here. Having the verb is the
prerequisite; deciding what "the verb matches the arm" means — and that a scroll
at the top is not a lie — is the next invariant, not this one.
