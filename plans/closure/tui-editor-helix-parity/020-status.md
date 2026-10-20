# Closure record — TUI editor Helix parity Milestone 020

Milestone: **M020 — the binding-set coherence guard, and §11's amended bullet**
Plan: `plans/implementation/tui-editor-helix-parity/020-binding-set-coherence-guard.md`
Roadmap: `plans/subsystems/tui-editor-helix-parity-roadmap.md` §7 (M020,
Amendments A1 and A2) and §11 (Amendment A1)
Closed: 2026-10-20
Baseline: `8df5dbb32` (`feat/auto-20261018-12255c45`)

## 1. The finding

The roadmap's §11 said, of the binding surfaces:

> The normal-mode keymap, the `:` palette, the help overlay, and `docs/tui.md`
> describe one identical set of bindings, and that set is derived from the
> binding table rather than hand-maintained beside it.

**That sentence could not be true, and — worse — it could not be false
either.** Nothing in the repository compared the code's bindings to the
documentation's, so the sentence described an aspiration with no test behind
it. Three findings, each measured:

1. **The help overlay it names is dead code.**
   `src/tui/components/help_overlay.rs` has no `mod` declaration in
   `src/tui/components/mod.rs` and no reference anywhere in `src/`. The live
   help is `src/tui/components/dialogs/help.rs`, and it carries no editor
   chords at all. A dead file cannot describe a set of bindings, and editing
   it would be editing a file nothing reads.
2. **The `:` palette is not the keymap.** `EditorCommandName::ALL` is a
   *command registry* — `w`, `q`, `wq`, `q!`, `e`, `wa`, `new`, `open`,
   `complete`. It was never the same set, so "one identical set" was never
   true of it.
3. **Nothing checked the document.** `grep` over `scripts/` for `docs/tui`
   returns no matches; no test reads the file. It is hand-maintained prose,
   rewritten by hand at each milestone's documentation step.

So the milestone did two things: it **amended the bullet** to the surfaces
that exist, and it **added the check** that makes the amended bullet true.

## 2. What was built

| Work package | Where |
|---|---|
| The guard | `scripts/check_tui_editor_doc_drift.py` (new, mode 755) |
| Its registration | `scripts/verify.sh quick`, `.github/workflows/ci.yml` |
| The document's markers | `docs/tui.md`: the two "not bound" lists |
| The amendment | `plans/subsystems/tui-editor-helix-parity-roadmap.md` §11 (Amendment A1) |
| The milestone entries | the same file's §7 (M019, M020) and §12 |

**No production code changed.** The milestone is infrastructure: a guard, two
markers in a document, and an amendment to a roadmap.

## 3. The guard

`scripts/check_tui_editor_doc_drift.py` reads three inputs and no more:

- `src/tui/commands/editor.rs`, through
  `scripts/check_tui_editor_key_authority.py` — for the claimed Normal-mode
  chords, the Insert-mode chords and the guard block;
- `src/tui/editor.rs`, through
  `scripts/check_tui_editor_leaf_authority.py` — for the five leaf tables and
  the prefix each is bound to;
- `docs/tui.md` — the document under test.

It reads the other two guards' **models** rather than re-parsing the dispatch,
for the reason the leaf guard already gave: two parsers for one dispatch
shape is two models of it, and the day they disagree one of them is wrong.

### 3.1 The five rules

| Rule | Direction | What it catches |
|---|---|---|
| 1 | code → doc | a leaf gesture the code binds and the document never mentions |
| 2 | code → doc | a chord the code claims and the document never mentions |
| 3 | doc → code | a key the document declares unbound that the code **binds** |
| 4 | structure | a document with no unbound declaration at all — the guard finding nothing because it looked in the wrong place |
| 5 | structure | a token in an unbound list that is not a recognisable binding shape |

**Rule 3 is the one that matters most.** A document that says a key does not
work is worse than one that omits it, and a key that gets implemented and
stays in the list is the drift that hurts. To make the claim checkable, the
document's two "not bound" lists gained a marker:

```markdown
<!-- unbound: the keys below are deliberately not bound -->
```

The marker is an HTML comment, so it costs a reader nothing, and it makes the
"not bound" claim checkable in **both** directions.

**Rule 4 exists because a guard that finds nothing because it looked in the
wrong place reports a clean tree and means nothing.** The guard fails when it
cannot find the structure it is checking.

### 3.2 The spellings the document uses

A guard that demanded one spelling of every key would be a style guard, and
it would fail on the document's own inconsistency: `docs/tui.md` writes
`Alt+d` in the Insert-mode section and `Alt-d` in the multi-selection
section, and spells the minus key `Alt-minus`. The guard accepts the
spellings the document actually uses:

- a plain chord is the character itself (`d`);
- a shifted chord is the upper-case character (`A`);
- `Ctrl` is `Ctrl+x` or `Ctrl-x`;
- `Alt` is `Alt+x` or `Alt-x`;
- the minus character is `minus` when it follows a modifier (`Alt-minus`);
- the **blank** is the space menu's first key, and the menu is named
  (`space`), because a code span cannot carry a blank and still be readable.

The leaf gestures have one spelling per prefix, and the prefix's own display
string decides it: `space` and `m` take a space (`space a`, `m m`); `g`, `[`
and `]` do not (`gs`, `[p`, `]Space`).

### 3.3 What the guard deliberately does **not** check

The document's prose contains bracketed identifiers (`[INS]`, `[NOR]`,
`[human_shell]`), a JSON path (`[theme].name`), a *negative* example
(`m z` is not a gesture), and the match-mode gestures that take a third key
(`m a (`, `m s (`). A reverse check over **every** code span in the document
reports **fifty** of those as defects, and a guard that reports defects that
are not defects gets disabled by the next person who hits one.

The reverse direction is therefore checked only where the document makes an
explicit, structured claim — the unbound lists. The rest is a residual
(§9).

## 4. Non-vacuity

### 4.1 The self-test

`--self-test` runs **eight** synthetic cases through the same analysis the
real tree goes through, and each case states which rule it proves:

| Case | Rule | What it proves |
|---|---|---|
| a clean tree finds nothing | — | a clean tree is not mistaken for a clean *check* |
| an undocumented leaf | 1 | the code → document direction fires |
| an undocumented chord | 2 | the keymap → document direction fires |
| a bound key declared unbound | 3 | the document → code direction fires |
| a document with no unbound declaration | 4 | the structure check fires |
| an unreadable unbound token | 5 | a list the guard cannot read is a list it cannot check |
| a spelling the document uses | — | `Alt-u` is accepted, so the guard is not a style guard |
| a shifted chord as its upper-case character | — | `A` documents the shifted chord |

### 4.2 The injection proof

`/tmp/m020_inject_guard.py` injects six defects into the real tree, shows
the guard failing **with its own rule**, and restores the tree
**byte-identically** — the backup is keyed by the path *relative* to the
repository root, because the daemon's cwd is wherever it was launched.

| Injection | Rule that fired |
|---|---|
| a new leaf glyph in `SPACE_LEAVES` | 1 |
| every mention of `space a` removed from the document | 1 |
| every mention of `x` removed from the document | 2 |
| `space a` declared unbound | 3 |
| every unbound marker removed | 4 |
| `g f` added to an unbound list | 5 |

**The first draft of this proof injected defects that were not defects**, and
the guard correctly passed them: it removed *one* mention of `space a` (the
document mentions the leaf in several places) and *one* of the two markers.
The proof now removes **every** mention and **every** marker. A proof that
does not inject a real defect proves nothing, and a guard that fails when it
should not is a broken guard — so both were fixed before the proof was
accepted.

## 5. Verification

Every number below is from the sweep against this tree
(`CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0`).

| Command | Result |
|---|---|
| `python3 scripts/check_tui_editor_doc_drift.py` | pass — **44 leaf gestures and 80 chords are documented, and every key declared unbound is unbound** |
| `python3 scripts/check_tui_editor_doc_drift.py --self-test` | **8 cases passed** |
| the injection proof | **6 injections, 6 rules, the tree restored byte-identically** |
| `python3 scripts/check_tui_editor_key_authority.py` | **2 guard-block / 71 Normal / 8 Insert** — unchanged |
| `python3 scripts/check_tui_editor_leaf_authority.py` | **5 tables / 44 glyphs / 12 bindings** — unchanged |
| `cargo fmt --all -- --check` | pass (re-run after `cargo fmt --all`; see M019's §5) |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | pass |
| `cargo test -p codegg --lib --locked` (whole lib) | **6842 passed / 3 failed / 4 ignored** — the three are the pre-existing load-sensitive classes, each passing alone |
| `bash scripts/verify.sh quick` | **"Quick verification passed."** — and the sweep it runs now includes this guard |

The eighty chords are the seventy-one Normal-mode claims, the two guard-block
claims and the eight Insert-mode claims, less the Insert-mode wildcard — the
claim that owns *every* printable character, which names no single binding to
document.

## 6. Exit conditions

| # | Criterion | Evidence |
|---|---|---|
| 1 | the guard exists, is registered in `verify.sh quick` and in the CI `verify` job, and passes on the tree it is written for | §2 and §5 |
| 2 | the self-test passes, and one of its cases proves a clean tree is not mistaken for a clean *check* | §4.1, the first case |
| 3 | the injection proof shows the guard failing on an injected defect and the tree restored byte-identically | §4.2 |
| 4 | the roadmap's §11 carries a dated Amendment A1 that names the two exceptions and the amended bullet | the roadmap's §11 |
| 5 | the key guard's and the leaf guard's counts do not move | **2 / 71 / 8** and **5 / 44 / 12** |

## 7. The amendment

The roadmap's §11 now carries a dated **Amendment A1 (2026-10-20)**, and the
amended bullet reads:

> The bindings the code owns and the bindings the documentation describes are
> one set: every leaf gesture and every Normal-mode chord the code claims is
> documented in `docs/tui.md`, every key the documentation declares unbound is
> unbound, and the check is mechanical — a guard script in the routine
> verification set. The `:` palette and the dead help overlay are named
> exceptions, recorded as such.

The amendment is a **narrowing**, and the narrowing is the point: it names the
two surfaces that exist and can be compared, and the two that do not. It does
not claim the document is prose-complete — the guard checks the *binding set*,
not the explanations.

The amendment was made under the standing rule that a roadmap is amended by an
explicit, dated block, never by a silent rewrite.

## 8. The documents that moved

| Document | What moved |
|---|---|
| `docs/tui.md` | the two "not bound" lists gained the marker, and the first one gained a reader-facing sentence saying the list is **checked** |
| `plans/subsystems/tui-editor-helix-parity-roadmap.md` | §11's Amendment A1; §7's M019 and M020 entries with their amendments; §12's two new rows |
| `.opencode/skills/tui/SKILL.md` | the guard table gained the doc-drift row — and the **apply-path** and **completion-authority** rows, which the table was missing entirely |

The skill and its `architecture/` doc moved in the same commit, as `AGENTS.md`
requires. `architecture/tui.md` did **not** need to change: the milestone
changed no production behaviour, and the guard is a verification surface
rather than a runtime contract.

## 9. Unresolved findings

| Severity | Finding | State |
|---|---|---|
| **medium** | the document's **prose** may advertise a key that does not exist, outside the "unbound" lists | **registered, not discharged.** A reverse check over every code span reports fifty defects that are not defects — the bracketed identifiers `[INS]`, `[NOR]` and `[human_shell]`, the JSON path `[theme].name`, and the negative example `m z` is not a gesture. The reverse direction is therefore checked only where the document makes an explicit, structured claim. Making it wider needs a way to mark a prose mention as an example, which is a document-shape decision. |
| **low** | the `Alt+` / `Alt-` spelling inconsistency in `docs/tui.md` | **registered, not fixed.** The guard accepts both spellings deliberately — it is a coherence guard, not a style guard. |
| **low** | the `m` grammar's multi-key gestures (`m a (`, `m s (`) are not in the guard's universe | **registered, not fixed.** The leaf tables hold the *first* key of each gesture; the second and third keys are arguments. A guard over the full grammar is a different guard. |

## 10. Carried forward

- **The change quartet's missing data source** — `]g`/`[g`/`]G`/`[G` still has
  none. It needs its own milestone and its own plan.
- **The dirty-buffer merge** — unchanged and still deferred.
- **The other LSP pickers show the first rows only**, and **the code-action
  read passes no diagnostics context** — both from M017.
- **M014** stays blocked on ADR-0034's acceptance — a decision the
  implementation is not free to make.

## 11. Roadmap disposition

**This is the last milestone of the roadmap**, and it is the one that made the
roadmap's own completion definition checkable.

With M019 and M020 closed, every bullet of §11 is met:

| §11 bullet | Status |
|---|---|
| every milestone through M015 closed, or the roadmap superseded | **met.** M001–M013, M015–M020 all closed with closure records. M014 is the **named** exception, blocked on ADR-0034. |
| the binding surfaces describe one set, derived from the binding table | **met, as amended** by Amendment A1 — and now **mechanically enforced** by M020's guard. |
| no declared binding is unreachable, and the reachability guard is in the routine set | **met** — six editor guards in `verify.sh quick` and in CI |
| the Insert-mode printable-character sweep passes non-vacuously across both keymaps | **met** (M011) |
| terminal-panel parity verified or an exemption recorded | **met** (M006/M007/M008; the `C` chord recorded as an exemption) |

The roadmap therefore **closes**, with its remainder explicitly owned
elsewhere: the §3 deferred list, the ADR-0034-gated slices, and the residuals
registered in §9 above.

## 12. Registry updates

- `plans/registry.md`: the M020 row moves to `closed`, with this record
  linked; the roadmap's own row moves to `closed`, and the residuals are
  registered in the registry's deferred section.
- The roadmap's §12 table gains an M020 row; the roadmap's status line moves
  to `closed`.
- The blocked-work audit found nothing to unblock: M014's blocker is
  ADR-0034's acceptance, and no registered plan lists M020 as a dependency.
  The residuals this milestone registered are **deferred product work**, not
  blocked milestones, and they are registered in the registry's deferred
  section rather than as new plans.
