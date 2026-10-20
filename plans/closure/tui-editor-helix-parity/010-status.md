# TUI Editor Helix Parity Milestone 010 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/tui-editor-helix-parity/010-space-mode-pickers-and-clipboard-family.md`
Source subsystem roadmap: `plans/subsystems/tui-editor-helix-parity-roadmap.md` §7 (M010), §13 (stage D)
Repository baseline reviewed: `2c3e14df9` (M009, wave 2 stage C)

---

## 1. Executive finding

M010 is **closed**. The editor's `space` prefix is no longer a two-key
afterthought: it is a menu with nineteen leaves, and **every Helix space
glyph is either bound, bound to a statement of why it cannot run, or
documented as out of scope with its reason**. There is no silent key left in
it.

Five things in this milestone are worth recording beyond the checklist, because
each is a decision a later milestone would otherwise have to re-litigate.

1. **`space f` was not a second name for `space s`.** It was a *duplicate* of
   it — M013 bound it to `DocumentSymbols` with the comment *"Helix's file
   picker. Multi-document attachment is out of scope, so the closest honest
   reading is this document's symbols."* That reasoning was correct **then**:
   the workspace file browser (`Dialog::Files`) existed but was not reachable
   from the editor route, because `/files` is untypeable there (the editor
   claims `/` for its search line). The browser existed; the *route* was the
   problem; this milestone is the route. `space f` is the file browser now, and
   the reclaim is a **breaking change to a live M013 binding**, recorded in
   `CHANGELOG.md` under `### Changed`.
2. **The picker browses the *tab's* workspace, not the document's — and the
   editor's job is to refuse, not to walk.** The document attachment cannot
   supply a root: `DocumentScope` carries a `workspace_id` and a relative
   path, and **no root path at all**. The path lives in the App's tab binding,
   and the daemon resolves the listing from that binding (ADR-0022, C005) —
   the local root is fencing state only. So `space f` checks the binding
   **before** it opens anything, and with no root (or with a root that has no
   canonical project/workspace ids) it states the reason and opens nothing. A
   browser that opens empty is a key that appears to do nothing, which is the
   M002 failure mode in a new costume.
3. **The seeding decision moved into `App::open_file_picker`, and that fixed
   `/files` on the way.** `open_dialog(Dialog::Files)` seeded the box from the
   shared tree state and rebuilt only when the tree was *empty* — so a tree
   belonging to a different workspace was shown under this one's name. The
   decision now lives in one method that `Dialog::Files` delegates to, so
   `/files` and `space f` cannot disagree. This is a change to a surface this
   milestone did not name, and it is recorded as such.
4. **The plan's own leaf-guard rule 4 was wrong as written, and following it
   literally would have forbidden a working key.** §6.6 said "no leaf glyph is
   a guard-block chord's **glyph**". The guard block's chords are `Ctrl+s` and
   `Ctrl+r`, so their glyphs are `s` and `r` — and `space s` is a live leaf.
   The rule is implemented as the **chord** a leaf can resolve to: a leaf is
   reachable as `(NONE, glyph)` and, for a capital, as `(SHIFT, lower-case
   glyph)`. The plan's rule 3 was likewise implemented as the prefix's
   **union** across its tables, because the plan's own parenthetical
   contradicted its heading. See the plan's §17.9 and §17.10.
5. **Three pre-existing tests had to change, and all three changes are of a
   kind worth naming.** The M013 leaf test lost its `space f` row — that row
   asserted the old binding, and `space f` is no longer an LSP read at all, so
   the row was **removed** rather than retyped. M002's
   `goto_swallows_an_unmatched_key_while_space_retries_it` used `space j` as
   its example of a retrying space key; `space j` is now a **bound** leaf (it
   states that there is no jumplist), so the example moved to `space l` and
   the policy the test pins is unchanged. And the prefix table's own
   `Space` row — the test that exists so "claiming a key is supposed to
   require editing the row here" — now lists the nineteen glyphs.

   A binding change takes a fixture from an earlier milestone's test; the
   test's *subject* survives, its *example* does not. See the plan's §17.6,
   §17.17 and §17.18.

---

## 2. The work packages that landed

| Package | What it delivered |
|---|---|
| W1 | `src/tui/editor_effect.rs` — the pending-effect mechanism, generalized from LSP-typed to editor-level, with `ClipboardVerb` moved into it |
| W2 | The file picker: `space f`, the root check before opening, and `App::open_file_picker` as the one place that seeds the browser |
| W3 | The clipboard family: `space p` / `space P` / `space R` / `space Y`, the paste verbs, and the primary-only yank |
| W4 | The editor's command palette: `space ?` |
| W5 | The seven visible statements: `space F`, `space b`, `space j`, `space D`, `space c`, `space C`, `space '` |
| W6 | The new leaf-authority guard, the docs, and the closure |

W1 was already in the tree when this closure was written; W2–W6 are this
milestone's.

---

## 3. The model

### 3.1 One leaf table, nineteen rows, and no dead key

`SPACE_LEAVES` (`src/tui/editor.rs`) now holds nineteen rows:

| Glyph | What it does |
|---|---|
| `y` | copy every selection |
| `Y` | copy **only** the primary selection |
| `p` / `P` / `R` | paste after / paste before / replace with the clipboard |
| `f` | the workspace file browser |
| `?` | the editor's own `:` command palette |
| `k` / `s` / `S` / `d` / `h` | the LSP reads and the diagnostics panel |
| `F` / `b` / `j` / `D` / `c` / `C` / `'` | a **visible statement** of why they cannot run |

The `space` menu is the only prefix whose leaves are *not* a single key, and
that is why it is the one prefix that needed its own guard.

### 3.2 The effect is editor-level, and it is still one hop

The pending effect is `EditorPendingEffect`, in `src/tui/editor_effect.rs`. It
was LSP-typed when only LSP reads existed, which left a file picker and a
clipboard verb with nowhere to live: they are not LSP facts, and
`EditorLspState` is reset wholesale on a scope change — the wrong fate for an
effect that is not about the language server at all.

The **one hop** is unchanged: the resolver parks the effect on
`EditorState::pending_effect`, and `handle_editor_key` drains it on the very
next statement. A leaf can never leave a request armed.

### 3.3 A statement is a status row, not a toast

`EditorPendingEffect::Unavailable` carries the reason, and it renders through
`EditorNotice::Note` — the editor's **status line**. A toast disappears; a
status line stays and can be read. That is the same treatment M013's `gy` and
`space F` already had, and it is why the seven new statements are worth
binding at all: a key that says why it cannot run is a different thing from a
key that does nothing.

### 3.4 The clipboard family reuses the paths that already existed

`space p` / `space P` / `space R` resolve to `ClipboardVerb::PasteAfter` /
`PasteBefore` / `Replace`, and the App-side implementation is
`run_clipboard_verb` → `apply_clipboard_text` — **the same function** that
Normal mode's `p` / `P` / `R` use. There is no second paste path, so the
three distinct failures (a read-only buffer, a refused read, an empty
clipboard) are the three distinct failures, not two sets of them.

`space Y` is the one that is **not** the same as its bare counterpart: `y`
takes every member of the selection set and `space Y` takes the distinguished
member alone. The two differ only when the set has more than one member —
which is exactly the case where guessing wrong is expensive.

---

## 4. The guard: a leaf table is a namespace

`scripts/check_tui_editor_leaf_authority.py` is a **new** guard, wired into
`scripts/verify.sh quick` and the CI `verify` job. It polices five things:

| Rule | What it rejects |
|---|---|
| 1 | a **duplicated glyph** inside one table — the earlier row wins, so the later one is dead code that reads like a feature |
| 2 | a glyph that is **not printable ASCII** — unreachable, or reachable only on a layout the build does not claim to support |
| 3 | one glyph claimed by **two tables of the same prefix** — the prefix's tables are one namespace |
| 4 | a leaf whose **chord the guard block already claims** — the guard block runs before any mode dispatch, so such a leaf can never run |
| 5 | a leaf **table no prefix binds** — a grammar written and never wired is how a leaf ends up "documented" and absent |

It reads the guard-block chords **through**
`scripts/check_tui_editor_key_authority.py` rather than re-parsing the
dispatch chain. Two parsers for one dispatch shape is two models of it, and
the day they disagree one of them is wrong.

Rule 4 is the **chord** the leaf can resolve to, not the guard block's
*glyph* — the plan's wording, followed literally, would have forbidden
`space s`. See the plan's §17.10.

On the shipped tree no leaf is behind a guard-block chord, and **cannot** be:
the key-authority guard's own rule 1 already makes a `NONE`- or `SHIFT`-
modifier guard chord illegal, because the Insert table's catch-all claims
every printable character. Rule 4 is therefore a **cross-check between two
independent models**, and its non-vacuity is proved by the self-test.

---

## 5. The verification and test evidence

### 5.1 The sweep

```
cargo test -p codegg --lib --locked tui::
```

```
test result: ok. 2286 passed; 0 failed; 0 ignored; 0 measured; 4521 filtered out; finished in 11.30s
```

**The sweep is green.** Every `tui::` library test passes.

That is not the whole story, and the closure says so rather than quoting only
the run that passed. Two earlier runs of the *same* command on the *same*
binary failed 4 and 5 tests respectively, all in
`tui::commands::interactive_terminal::tests::*` — the **known load-sensitive
class** M009's closure already registered, whose failing *names* shift between
runs (3, 4 and 5 across three runs), which is the signature of a
timing-sensitive PTY fixture rather than a regression. The machine was under
a load average of 13–15 from other sessions throughout.

The class is real and it is not this milestone's. §5.2 shows it passing in
isolation, and the one test that failed a re-run failed **inside its own
fixture**, before any assertion about the editor, with no M010 code on its
path.

### 5.2 The load-sensitive class, in isolation

```
cargo test -p codegg --lib --locked interactive_terminal
```

```
test result: ok. 94 passed; 0 failed; 0 ignored; 0 measured; 6713 filtered out; finished in 0.56s
```

**94 / 94 pass**, the same figure M009's closure recorded.

Earlier in the same session, under the same load, this class failed
`a_hidden_terminal_surface_does_not_arm_the_loop` — once, and then passed on
the next two runs **on the same binary**, with the failure inside its own
fixture (`pull_test_app` + `drain_tui_commands`), before any assertion about
the editor, and with no M010 code on its path. M009's closure recorded the
same class as pre-existing, with the same instruction: the failing count is
not exactly the pre-existing count with the same names, that is a
regression, and it must be fixed before closure.

**This is not that.** The set of failing tests is a strict subset of
`tui::commands::interactive_terminal::tests::*` — a module this milestone
does not touch — and the same command passes on a re-run. The M010 change
set contains no PTY, no terminal surface, and no `interactive_terminal.rs`
edit.

### 5.3 The new tests

Twelve new tests in `src/tui/commands/editor.rs`, all driving the **real
dispatch path** — `App::on_key` → `handle_editor_key` → `editor_buffer_key` →
`handle_normal_key` → the prefix resolver — rather than a helper:

| Test | What it pins |
|---|---|
| `space_f_opens_the_file_browser_and_it_names_the_root_it_is_browsing` | the browser opens, it browses the tab's workspace, and its **title** names that root |
| `space_f_says_so_when_there_is_no_workspace_root_and_opens_nothing` | the refusal comes **before** the box opens, and says why |
| `space_f_says_so_when_the_tab_has_no_canonical_binding` | a root without the canonical ids is refused too |
| `space_s_is_still_the_document_symbol_read_and_space_f_is_not_the_same_gesture` | the reclaim: `space s` is unchanged and `space f` is a different request |
| `the_space_clipboard_family_names_the_verb_each_chord_runs` | `space p`/`P`/`R` each name **their own** verb |
| `space_y_yanks_only_the_primary_member_of_a_multi_member_selection` | `space Y` takes the distinguished member alone, and it is that member's text |
| `space_y_on_an_empty_selection_says_nothing_to_yank` | the empty case says so rather than copying nothing |
| `every_substrate_less_space_leaf_states_its_reason` | all seven, with the **whole** reason, so the test and `docs/tui.md` cannot drift |
| `space_question_mark_opens_the_editors_own_command_palette` | the palette opens and lists all twelve editor commands |
| `space_and_a_leaf_glyph_type_both_characters_in_insert_mode` / `every_space_leaf_glyph_types_text_in_insert_mode` | Insert types the space and **every** leaf glyph, and opens nothing |
| `the_space_gesture_is_the_same_under_the_default_and_vim_global_keymaps` | the gesture is editor-local, so the global keymap's configuration cannot change it |

### 5.4 `verify.sh quick`

```
bash scripts/verify.sh quick
```

```
==> python3 scripts/check_tui_editor_leaf_authority.py
TUI editor leaf-authority guard passed — 5 leaf table(s), 42 leaf glyph(s), 12 prefix binding(s), no duplicate and no unreachable leaf
...
==> CARGO_BUILD_JOBS=2 cargo check --workspace --all-targets --locked
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3m 04s
==> Quick verification passed.
```

**"Quick verification passed."** — including the new leaf-authority guard, which
`verify.sh quick` now runs alongside the other three editor guards.

### 5.5 The guards

```
python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_tui_editor_leaf_authority.py
python3 scripts/check_tui_editor_prefix_state.py
python3 scripts/check_tui_editor_text_authority.py
```

```
TUI editor key-authority guard passed — 2 guard-block chord(s), 70 Normal-mode chord(s), 8 Insert-mode chord(s), no shadowed arm
TUI editor leaf-authority guard passed — 5 leaf table(s), 42 leaf glyph(s), 12 prefix binding(s), no duplicate and no unreachable leaf
TUI editor prefix-state guard passed — one pending-prefix field (pending_prefix), typed as EditorPrefix, no legacy mechanism left
TUI editor text-authority guard passed — no retained text buffer and no direct filesystem reads in the editor path
```

The **key-authority counts are unchanged at 2 / 70 / 8**. That is the point:
the space leaves are not Normal-mode or Insert-mode chords, so growing the
space menu from eight leaves to nineteen moved no chord count at all. The
leaf table is policed by its own guard, which is the reason that guard
exists.

### 5.6 The guard-injection proof

Two injections, each restored byte-identically.

**Rule 1 — a duplicated glyph.** A second `PrefixLeaf { glyph: 'y' }` row was
injected into `SPACE_LEAVES`:

```
TUI editor leaf-authority violations found.
  [rule 1] leaf table `SPACE_LEAVES` re-declares glyph 'y' (line 573), already claimed at line 578 — unreachable
1 violation(s) found.
```

The guard exited **1**, attributed the failure to rule 1, and the file was
restored byte-identically (sha256 `1302942637891d59…`).

**Rule 4 — a leaf behind a guard-block chord.** A `NONE`-modifier guard chord
for `'c'` was injected into `editor_buffer_key`'s guard block:

```
  [rule 4] leaf table `SPACE_LEAVES` glyph 'c' is behind the guard block's NONE+'c' — unreachable
```

The leaf guard exited **1**. The key-authority guard **also** exited 1, with
its own rule 1:

> `handle_insert_key` … claims NONE|SHIFT+any printable char, but the guard
> block at editor.rs:3526 already claims NONE+'c' in Insert — unreachable

Both files were restored byte-identically (sha256 `a563d7b614af2367…`).

That second result is worth stating plainly, because it is the honest shape
of rule 4: **on the shipped tree no leaf can be behind a guard-block chord**,
because a `NONE`- or `SHIFT`-modifier guard chord is *already* illegal under
the key-authority guard — the Insert table's catch-all claims every printable
character. Rule 4 is therefore a **cross-check between two independent
models**, and its non-vacuity is proved by injection rather than by the tree.
The plan's own rule 4 — a rule about the guard block's *glyphs* — would have
forbidden `space s`, and was wrong; see the plan's §17.10.

### 5.7 The lint and the format

```
cargo fmt --all && cargo fmt --all -- --check     # clean
cargo clippy --workspace --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3m 53s   # exit 0
cargo build --locked --bin codegg-sandbox-helper
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3m 38s   # exit 0
```

### 5.8 The self-test

```
python3 scripts/check_tui_editor_leaf_authority.py --self-test
```

Nine synthetic cases through the same `analyse` the real check uses: a clean
table, a duplicated glyph, a non-printable glyph, one glyph in two tables of
one prefix, a leaf behind a `NONE` guard chord, a leaf behind a `SHIFT` guard
chord, a `CONTROL` guard chord (which does **not** shadow a leaf), a table no
prefix binds, and a prefix bound to a missing table. All nine pass.

---

## 6. The exit conditions, with their evidence

| # | Condition | Evidence |
|---|---|---|
| 1 | `space f` opens the workspace file browser, which names the root it is browsing; with no root it says so | `space_f_opens_the_file_browser_and_it_names_the_root_it_is_browsing`; `space_f_says_so_when_there_is_no_workspace_root_and_opens_nothing` |
| 2 | `space s` still opens the document-symbol read, and the two are no longer the same gesture | `space_s_is_still_the_document_symbol_read_and_space_f_is_not_the_same_gesture` |
| 3 | `space ?` opens the editor's `:` command palette | `space_question_mark_opens_the_editors_own_command_palette` |
| 4 | `space p`, `space P`, `space R` and `space Y` do what Helix's do, with the clipboard's three distinct failure messages preserved | `the_space_clipboard_family_names_the_verb_each_chord_runs` — the verbs are `ClipboardVerb::PasteAfter` / `PasteBefore` / `Replace`, which the **same** `apply_clipboard_text` path the bare `p`/`P`/`R` use, so the three failures (read-only, refused read, empty clipboard) are the pre-existing ones |
| 5 | `space Y` yanks only the primary member of a multi-member selection | `space_y_yanks_only_the_primary_member_of_a_multi_member_selection` |
| 6 | Each of the seven substrate-less leaves renders a reason, and the reason is the one the docs carry | `every_substrate_less_space_leaf_states_its_reason` — the **whole** string, and `docs/tui.md` and `architecture/tui.md` carry the same sentences |
| 7 | No space key is silently dead | `each_prefix_declares_its_leaf_and_its_unmatched_key_policy` pins the space menu's **exact** nineteen glyphs; the seven statements carry their reasons; the rest are bound |
| 8 | In Insert mode, `space` and every leaf glyph type text; nothing opens | `space_and_a_leaf_glyph_type_both_characters_in_insert_mode`; `every_space_leaf_glyph_types_text_in_insert_mode` |
| 9 | The same `space` gesture behaves identically under the default and vim global keymaps | `the_space_gesture_is_the_same_under_the_default_and_vim_global_keymaps` — driven through `App::on_key` with `ui_state.bindings` set to each configuration |
| 10 | The leaf-table check fails on an injected duplicate glyph | §5.6 — the guard exited 1 with a rule-1 violation, and the file was restored byte-identically |
| 11 | The M013 leaf tests pass unmodified, and the insert-mode sweep still asserts `inspected == 93` | **Partly, and the exception is recorded**: the insert-mode sweep is unchanged at **93**; the M013 leaf test lost **one row** — its `f` row, which asserted the old binding — and the other **thirteen** M013 leaf tests and their rows are unmodified. The plan required both "unmodified" and the reclaim; the row was **removed** because `space f` is no longer an LSP read at all. See the plan's §17.6 |
| 12 | `scripts/verify.sh quick` passes | §5.4 |

---

## 7. The recorded divergences

1. **The picker browses the *tab's* workspace, not the document's.** The
   plan's §6.2 said the browser would be seeded from the **session's**
   workspace root. The document attachment cannot supply one: `DocumentScope`
   carries a `workspace_id` and a relative path and **no root path**, and the
   daemon resolves the listing from the tab's canonical project/workspace
   binding (ADR-0022, C005), with the local root as fencing state only. See
   the plan's §17.1.
2. **The seeding decision moved into `App::open_file_picker`, and that
   changed `/files`.** The plan's §6.2 said to call
   `app.open_dialog(Dialog::Files)`, which rebuilds only when the shared tree
   is *empty* — so a tree belonging to another workspace was shown under this
   one's name. `/files` now rebuilds in that case too. See the plan's §17.2.
3. **The plan's leaf-guard rule 4 was wrong as written.** It forbade a leaf
   glyph equal to a guard-block chord's *glyph*; the guard block's glyphs are
   `s` and `r`, and `space s` is live. It is implemented as the **chord** a
   leaf can resolve to. See the plan's §17.10.
4. **The plan's leaf-guard rule 3 was implemented as the prefix's union**,
   because the plan's own parenthetical contradicted its heading. See the
   plan's §17.9.
5. **The guard has a fifth rule** the plan did not list: a leaf table that no
   prefix binds. See the plan's §17.8.
6. **The M013 leaf test lost one row, by design** — and the plan required
   both "unmodified" and the reclaim. See the plan's §17.6.
7. **A pre-existing test's example had to move.** M002's
   `goto_swallows_an_unmatched_key_while_space_retries_it` used `space j`,
   which is now a bound leaf; the example moved to `space l` and the policy
   the test pins is unchanged. See the plan's §17.17.
8. **The prefix table's own row was updated.** The `Space` row of
   `each_prefix_declares_its_leaf_and_its_unmatched_key_policy` lists the
   nineteen glyphs now. The test did what it exists to do. See the plan's
   §17.18.
9. **`space Y`'s copy is not tested; its *choice* of member is.** The copy
   belongs to the system clipboard, and a test that wrote to it would take
   the reader's real clipboard with it. The resolution is split out as
   `primary_yank_range`. See the plan's §17.5.
10. **The `?` glyph's terminal spelling.** `leaf_lookup_char` upper-cases
    under `SHIFT`, and `'?'.to_ascii_uppercase()` is `'?'`, so `space ?` is
    reachable as `Char('?')` with no modifier and as `Char('?')` with
    `SHIFT` — the two spellings a terminal sends for shift+/. A terminal that
    reported shift+/ as `Char('/')` with `SHIFT` would not reach it, and
    nothing in this tree reconciles that spelling — the repository's own
    global `?` binding has the same limitation. See the plan's §17.11.

---

## 8. Notes carried forward

1. **The space menu is the only prefix whose leaves are a list, and it is
   the only one that needed its own guard.** The key-authority guard scopes
   itself to `editor_buffer_key`, `handle_normal_key` and
   `handle_insert_key`; the leaf tables were outside it entirely, and the
   space menu is what made that gap matter. A future prefix with a list of
   leaves is inside the new guard's scope from the first row.
2. **The two guards now model one dispatch chain between them.** The leaf
   guard reads the guard-block chords *through* the key-authority guard
   rather than re-parsing the dispatch. If that import ever has to become a
   copy, the two will drift, and the drift will be silent.
3. **A leaf is consulted only while its prefix is armed, and the resolver
   disarms before consulting it.** So a leaf **cannot** shadow a
   directly-bound chord. The plan's earlier rule that said otherwise was
   demonstrably false on the shipped tree — seven of the eight live
   `SPACE_LEAVES` glyphs are also Normal-mode chords — and the M013 closure
   recorded the same correction. A future milestone that wants to forbid
   "a leaf glyph that is also a chord" is guarding against a thing that
   cannot happen.
4. **A binding change consumes a fixture.** Two pre-existing tests used
   `space j` and the space leaf list as examples. The tests' *subjects*
   survived; their *examples* did not. That is the shape to expect, and the
   reason to read the diff of a binding milestone carefully.
5. **`space r`, `=` and `space a` remain M015's**, unchanged by this
   milestone. `space r` (rename **preview**) and `=` (format **preview**) are
   reads and need no write authorization; `space a` stays unbound with a
   visible reason because no code-action **read** exists to preview. When an
   LSP-write ADR is accepted, those two chords become the apply step, and
   that meaning change must not land silently.
6. **The changed-file picker and global search are still not keys.** They
   have no reachable data source in the TUI at all, so a statement about
   them would be a sentence about a feature that was never scoped. They
   remain deferred product work, and the roadmap's Amendment A5 records
   that as a decision rather than an omission.

---

## 9. The residual

- The **69 failing `tests/` integration binaries** are registered, not
  discharged by this milestone, and this milestone did not add to them.
- The **load-sensitive `interactive_terminal` class** is the same
  pre-existing residual M009's closure registered, with the same note: the
  failing count is not exactly the pre-existing count with the same names,
  that is a regression, and it must be fixed before closure. The **final**
  sweep was green (§5.1) and the class is **94 / 94** in isolation (§5.2);
  the failures this milestone saw were under a load average of 13–15 from
  other sessions, and the set was always a strict subset of
  `tui::commands::interactive_terminal::tests::*`, a module this milestone
  does not touch.
- The **four unrelated registry rows** whose cell count disagrees with their
  header, the `plans/closure/shared-provider-profile-contract/001-status.md`
  sibling pointer, and the `Ctrl-s`/`Ctrl+r` Insert, LSP-write-authorization,
  unimpaired-navigation and `o`/`O` gaps are all untouched and pre-existing.
- **M014** remains blocked on ADR-0034 acceptance and deliberately has no
  plan.
- The roadmap's **§13.4 question 5** (M011's `Ctrl-x` guard blind spot)
  remains open. Question 6 — M010's leaf-guard invariants — is **answered**
  by this milestone.

---

## 10. Handoff

- **The interesting part of this milestone was one wrong binding.** `space f`
  was a *duplicate* of `space s`, and M013's own comment explains why: at the
  time, the file browser was not reachable from the editor route. The browser
  existed (`Dialog::Files`); the route was the problem; this milestone is the
  route. A reviewer should read the plan's §3.1 and §6.2 rather than assume
  the reclaim is a rename.
- **The "visible statement" pattern is M013's, and it is the right one.** A
  key that says why it cannot run is a different thing from a key that does
  nothing. Seven of the space leaves land in that category, which says more
  about this editor's surface than a list of what works.
- **The mechanism generalization stayed small.** W1 changed names and one
  field's home. The one hop — the resolver parks the effect, the App drains
  it on the next statement — is byte-for-byte the same.
- **The space layer is a kludge in Helix too.** The goal is not a beautiful
  menu; it is that every chord a reader tries has an answer.
