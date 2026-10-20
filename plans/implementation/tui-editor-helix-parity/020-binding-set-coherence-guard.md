# TUI editor Helix parity Milestone 020 — the binding-set coherence guard

Status: implemented - closed 2026-10-20
Closure record: `plans/closure/tui-editor-helix-parity/020-status.md`
Repository baseline: `8df5dbb32` (`feat/auto-20261018-12255c45`)

Source roadmap: `plans/subsystems/tui-editor-helix-parity-roadmap.md` §11 (the
completion definition this milestone serves) and §7 (a new milestone added by
the M020-planning amendment; see the roadmap's Amendment A4)
Long-term requirements: `plans/000-long-term-specification.md` (the editor's keymap
is a contract, not an implementation detail)
Applicable ADRs: none
Primary class: infrastructure

## 1. Objective

Make the roadmap's completion definition **checkable**, and check it.

The roadmap's §11 says, of the binding surfaces:

> The normal-mode keymap, the `:` palette, the help overlay, and `docs/tui.md`
> describe one identical set of bindings, and that set is derived from the
> binding table rather than hand-maintained beside it.

Today that sentence cannot be true, and — worse — it cannot be **false**
either. Nothing in the repository compares the code's bindings to the
documentation's, so the sentence describes an aspiration with no test behind it.

## 2. Why the sentence cannot be true as written

Three surfaces are named, and each is a name the sentence gets wrong:

1. **The help overlay** — `src/tui/components/help_overlay.rs` — is
   **unreferenced dead code**. It has no `mod` declaration in
   `src/tui/components/mod.rs` and no reference anywhere in `src/`. The live
   help is `src/tui/components/dialogs/help.rs`, and it carries no editor
   chords at all. A dead file cannot describe a set of bindings, and editing
   it would be editing a file nothing reads.
2. **The `:` palette** — `EditorCommandName::ALL` (`src/tui/editor.rs:1445`) —
   is a **command registry** (`w`, `q`, `wq`, `q!`, `e`, `wa`, `new`, `open`,
   `complete`), not the keymap. It was never the same set, so "one identical
   set" was never true of it.
3. **`docs/tui.md`** is **hand-maintained prose**, rewritten by hand at each
   milestone's documentation step. No guard script references it: `grep` over
   `scripts/` for `docs/tui` returns no matches, and no test reads the file.

So the bullet names surfaces that do not exist in the form it describes, and
there is no mechanism that could satisfy it. This milestone **amends the bullet
to the surfaces that exist** and **adds the mechanism**.

## 3. What the amendment says

The §11 bullet is replaced by an **Amendment A4** block, dated, with the reason
recorded — the standing rule for amending a roadmap is an explicit amendment on
the milestone entry, never a silent rewrite. The amended bullet:

> The bindings the code owns and the bindings the documentation describes are
> one set: every leaf gesture and every Normal-mode chord the code claims is
> documented in `docs/tui.md`, every key the documentation declares unbound is
> unbound, and the check is mechanical — a guard script in the routine
> verification set. The `:` palette and the dead help overlay are named
> exceptions, recorded as such.

The amendment is a **narrowing, and the narrowing is the point**: it names the
two surfaces that exist and can be compared, and it names the two that do not.
It does not claim the doc is prose-complete; the guard checks the *binding set*,
not the explanations.

## 4. The guard

`scripts/check_tui_editor_doc_drift.py`.

It reads the code the other guards already read, and the one document that
describes the bindings:

- `scripts/check_tui_editor_key_authority.py` — imported as a module, for the
  claimed Normal-mode chords, the Insert-mode chords and the guard block. The
  completion guard already reads that guard's *model* rather than its output,
  and this guard does the same: one analysis, no second parser.
- `scripts/check_tui_editor_leaf_authority.py` — imported as a module, for the
  five leaf tables and the prefix each is bound to.
- `docs/tui.md` — the document under test.

### The rules

| Rule | Direction | What it catches |
|---|---|---|
| 1 | code → doc | a leaf gesture the code binds and the document never mentions |
| 2 | code → doc | a Normal-mode chord the code claims and the document never mentions |
| 3 | doc → code | a key the document declares unbound that the code **does** bind |
| 4 | structure | a document with no "unbound" declaration at all — the guard finding nothing because it looked in the wrong place |

Rule 3 is the one that needs the document to *say* which keys are unbound, and
that is a small, explicit change to `docs/tui.md`: the two lists that already
exist gain a marker.

```markdown
<!-- unbound: the keys below are deliberately not bound -->
- `gf`, `ga`, `gm`, `gn`, `gp` — files and alternate buffers. …
```

The marker is a comment, so it costs the reader nothing, and it makes the
"not bound" claim **checkable in both directions**: a key that gets implemented
and stays in the list is caught by rule 3, which is the drift that matters
most, because a document that says a key does not work is worse than one that
omits it.

Rule 4 exists because a guard that finds nothing because it looked in the
wrong place is **worse than no guard**: it reports a clean tree and means
nothing. The guard fails when it cannot find the structure it is checking.

### What the guard deliberately does **not** check

The document's prose contains bracketed identifiers (`[INS]`, `[NOR]`,
`[human_shell]`), a JSON path (`[theme].name`), and the match-mode gestures that
take a third key (`m a (`, `m s (`). A reverse check over *every* code span in
the document reports fifty of those as defects, and a guard that reports
defects that are not defects gets skipped. The guard therefore checks the
reverse direction only where the document makes an explicit, structured claim —
the "unbound" lists — and records the rest as a residual.

## 5. The spelling rules

A guard that demands one spelling of every key would be a style guard, and it
would fail on the document's own inconsistency: `docs/tui.md` writes `Alt+d` in
the Insert-mode section and `Alt-d` in the multi-selection section, and spells
the minus key `Alt-minus`. The guard accepts the spellings the document
actually uses and says so in the script:

- a plain chord is the character itself (`d`);
- a shifted chord is the upper-case character (`A`);
- `Ctrl` is `Ctrl+x` or `Ctrl-x`;
- `Alt` is `Alt+x` or `Alt-x`;
- the minus character is `minus` when it follows a modifier (`Alt-minus`).

The leaf gestures have one spelling per prefix, and the prefix's own display
string decides it: `space` and `m` take a space (`space a`, `m m`), `g`, `[`
and `]` do not (`gs`, `[p`, `]Space`).

## 6. Required tests

- a `--self-test` with at least six cases, each asserting **which** rule fired,
  including a case where an injected defect fires *no* rule — the
  non-vacuity case. **Eight** were written: the six the plan named, plus the
  two that prove the guard is not a *style* guard (a spelling the document
  actually uses is accepted, and a shifted chord is documented as its
  upper-case character).
- an injection proof: a script that edits the tree, shows the guard failing
  **with its own rule**, and restores the tree **byte-identically** (the
  backup keyed by relative path, because the daemon's cwd is not the
  worktree's). **Six** injections, one per rule.

**The first draft of the injection proof was wrong, and the guard was right.**
It removed *one* mention of `space a` and *one* of the document's two unbound
markers — and the guard passed both, correctly, because the document still
mentioned the leaf elsewhere and still carried the other marker. The proof now
removes **every** mention and **every** marker. A proof that does not inject a
real defect proves nothing, and a guard that fails when it should not is a
broken guard.

## 7. Required verification commands

```bash
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0
python3 scripts/check_tui_editor_doc_drift.py
python3 scripts/check_tui_editor_doc_drift.py --self-test
python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_tui_editor_leaf_authority.py
bash scripts/verify.sh quick
```

## 8. Acceptance criteria

1. The guard exists, is registered in `scripts/verify.sh quick` and in
   `.github/workflows/ci.yml`, and passes on the tree it is written for.
2. Its `--self-test` passes, and one of its cases proves a clean tree is not
   mistaken for a clean *check*.
3. The injection proof shows the guard failing on an injected defect and the
   tree restored byte-identically.
4. The roadmap's §11 carries a dated Amendment A4 that names the two
   exceptions and the amended bullet.
5. The key guard's and the leaf guard's counts do not move: **2 / 71 / 8** and
   **5 / 44 / 12**.

## 9. Stop conditions

Report rather than improvise if:

- the guard cannot be made to pass on the tree it is written for without
  changing a *binding* — a document that needs a binding to change to satisfy
  a document check has found a real disagreement, and that is a finding to
  report, not a defect to paper over;
- the guard would need to parse the document's prose to tell a binding from an
  identifier, which is the fragility this milestone exists to avoid.

## 10. Closure evidence required

- the self-test's case list, with the rule each case fires;
- the injection proof's output;
- the guard's output on the tree, and the counts from the other two guards.

## 11. Handoff notes

- This milestone changes **no production code**. It adds a guard, two markers in
  a document, and an amendment to a roadmap.
- The guard is registered in the *routine* verification set, so a binding that
  is not documented fails the ordinary sweep — which is the whole point of the
  completion definition being checkable.
- The residual — the document's prose advertising a key that does not exist,
  outside the "unbound" lists — is recorded, with the reason it is not
  checked: fifty false positives on the current document, and a guard that
  cries wolf is disabled by the next person who hits one.
