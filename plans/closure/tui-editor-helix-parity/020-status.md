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

## Registry narrative

Moved verbatim from the `plans/registry.md` row for **TUI editor Helix keybinding parity** on 2026-10-23, so that the registry stays an index.

This is the **explicitly deferred successor** named in `plans/subsystems/tui-editor-correctness-usability-roadmap.md` §3 ("full vim/Helix parity … deferred successor workstreams"); it does not reopen that closed roadmap or its milestones. Baseline `719cfeb5d`. Survey of the actual dispatch tables (not the docs) found: **`Ctrl+r` redo is unreachable** — `editor_buffer_key`'s mode-independent resync guard returns `RequestResync` before `handle_normal_key`'s `editor_redo` arm, and that function has exactly one call site, so Normal mode has undo (`u`) and no reachable redo; the three existing redo tests call `editor_redo` directly and therefore never exercise the key path, which is why the suite is green. **M001 is closed:** redo is now `U` (Helix's own chord), `Ctrl+r` keeps exactly one owner in both modes, a second dead-alternative instance (`\| (m, Char('d'))` in the `D` arm) was also closed, and `scripts/check_tui_editor_key_authority.py` plus dispatch-path tests make the class non-recurring. **Both bespoke prefix mechanisms are now gone** — M002 replaced `pending_command: String` and `yank_prefix_pending: bool` with one `EditorPrefix` enum field, so `[`/`]` are armed against a real grammar and a future `Alt-*` or `f`-capture grammar adds a leaf rather than a flag. **M003 used exactly that leaf path:** `f`/`F`/`t`/`T` are `EditorPrefix` variants with no presentation field of their own — their next key is *data* (the target character) rather than a leaf glyph. The find scan is bounded and **discloses** when it stops short (`no 'z' within the 2 MiB scan bound`) rather than reporting a truncated result as absent. Two real defects were found and fixed: a per-byte scan that could not read a byte inside a multi-byte character (`read_bytes` refuses a non-boundary range, so it walked past every multi-byte target — 79 s → 0.22 s once chunked), and an M001 test asserting `f`/`t`/`F`/`T` were unclaimed. **M003 does not fully distinguish `t`/`T` from `f`/`F`:** per the plan's AC2 they land on the target, which at this milestone makes them cursor-equivalent — the real distinction is a selection property and belongs with M007. Recorded, not papered over. **Planning pass 2026-10-18 added M013 after verifying two LSP delivery defects at baseline `84440fe7f`:** the diagnostics publisher has zero callers (so F2 can never return contents) and a multi-server workspace root resolves to a false `NotReady` (so F3–F9 are dead in any polyglot project). M013 is now a hard prerequisite of M010; binding LSP keys before it lands would ship always-`NotReady` panels. ~~`SelectionState` holds one `Option<Selection<P>>`, never a set, so ~15 Helix selection verbs are ~~~~unimplementable as written rather than merely unbound.~~ **Resolved by M007** (closed 2026-10-18): `SelectionState` now holds `Vec<Selection<P>>` plus a `primary` index, and the single-selection accessors were kept **primary-scoped** rather than replaced, so the terminal panel kept running the same rules with no call site changed. **No tree-sitter exists anywhere in the workspace**, so every `TS`-marked Helix binding is blocked at the dependency level — gated on proposed ADR-0034. `LspReadOperation` stays read-only by construction; M013 extends it with read variants and rename/format **preview** only, while `=` format / `space r` rename apply / `space a` code-action apply remain deferred pending an authorization ADR. No paste binding existed at all until **M004**, which is now closed.** M004 shipped `p`/`P` paste, bare `y`, `r`, `R`, `~`, `` ` `` and `` Alt-` ``: a user can now move text (`y` then `p`), which was impossible before. It corrected the live Helix keymap against the M004 implementation plan, which had `R` as vi's "replace to end of line" — Helix binds it to `replace_with_yanked`, taking no argument and entering no Insert mode, so the plan's whole retained-tail design was dropped (amendment A1). `R` was already bound to reload as a Normal-only **guard-block** chord, and that chord is exactly what `scripts/check_tui_editor_key_authority.py` rejects as shadowing a Normal-mode verb, so it and `BufferKey::RequestReload` are gone; **reload survives as `:e`**, so a keystroke moved but no capability was lost. There is no editor-local register: `y`/`space y` both write the OS clipboard, which is why `p` pastes text from any application. M009 is blocked on **ADR-0034 acceptance alone** — its M008 edge was discharged by M008's closure on 2026-10-19, and a proposed ADR confers no authorization, matching the ADR-0030 precedent. **Planning pass 2026-10-18 (later the same day) wrote plans for M006, M007 and M008 and produced nine corrections against this roadmap's own prose.** The three that matter: M006's boundary was **claiming `]d`/`[d`/`]D`/`[D` twice** — M013's boundary already named "]`/`[` diagnostic navigation" — and also described `]D`/`[D` and `]G`/`[G` **backwards** (`]` is *forward* throughout the layer, so `]D` is last and `[D` is first); those four bindings are now **M013's**. M006's **change** quartet (`]g`/`[g`/`]G`/`[G`) has **no reachable data source** — `egggit`'s `ChangedFile`/`FileDiff` carry no line ranges and the only parsed hunk type, `DiffHunk`, is behind a private widget method — so the roadmap's "reuse the existing diff surface" is false and it is registered as deferred product work needing a milestone of its own. M007's entry wrongly named **`D` as selection-dependent**: `resolve_edit` never calls `selection_range`, so `D` stays cursor-only rather than gaining a new semantic on a local extension. M008 was registered **blocked on M007**, which `plans/003-planning-process.md` §4 makes mandatory; it was written early only to freeze the set API contract M007 must expose. **That write-out paid off** — the contract shipped as pinned and M008 went `ready` with no corrective pass. **M008 is now closed** (`plans/closure/tui-editor-helix-parity/008-status.md`, 2026-10-19): all eighteen verbs are bound, the guard moved from 48 to 66 Normal-mode chords, and the `:` command line was reused for the regex verbs, so `check_tui_editor_prefix_state.py` emits byte-identical output before and after — no new prefix field, no new input mode. Two real defects were found by the milestone's own tests and fixed: `K` with no matches refused instead of emptying the set, and `C` at end-of-buffer reported success while adding a cursor on the member's own byte, because `len_lines()` counts a trailing line no byte offset resolves into. **Terminal-panel parity broke on exactly one chord and is recorded, not fixed:** the panel claims `C`/`c` for `Clear` while the editor uses `C` to add a cursor. ~~M008 is now `ready` with no corrective pass.~~ **M011 is now closed** (`plans/closure/tui-editor-helix-parity/011-status.md`, 2026-10-19): the nine Helix insert-mode chords are bound with exactly one owner each — the insert table became a tuple match on `(modifiers, code)` — the two mode-blind panels (the completion popup and the LSP diagnostics panel) no longer claim a bare character or `Enter` in Insert mode, and a typing burst is now **one** undo step through an insert session that commits when insert mode ends. `Ctrl+s`/`Ctrl+r` keep their mode-independent save/resync meaning; Helix's insert-mode `Ctrl+s` (undo checkpoint) and `Ctrl+r` (register) are declined, and the checkpoint capability is delivered by the coalescing instead. The guard's Insert chord count is **8**, not the 9 this roadmap's §13 originally recorded — the guard counts character claims, and two of the nine are key-code claims while `Ctrl+x` lives in a stage the guard does not scan; the Normal-mode count is unchanged at 66, which is the number M012 pins. Four existing tests changed because they asserted the old per-keystroke granularity. **M015 closed 2026-10-19 (roadmap Amendment A1):** the two LSP previews M013 shipped now have keys — `=` shows the format preview, `space r` collects a new name on the `:` command line and shows the rename preview — both **apply nothing**, and `space a` states that no code-action request exists. The chords cannot reach ADR-0008's accepted LSP write path either — they produce a *read result*, not a registered reviewable artifact — so the gap is the **bridge**, not an ADR. The preview/apply transition is registered now, in the roadmap, the closure record and the `CHANGELOG.md`, so the future milestone does not change a chord's meaning silently. The guard's Normal-mode chord count is **71** — one for the `=` arm — and the leaf table grew by two glyphs (`r`, `a`), so the leaf-authority guard reports 5 tables / 44 glyphs / 12 bindings. **M016 is `ready`** (`plans/implementation/tui-editor-helix-parity/016-preview-apply-bridge.md`): the bridge the M015 closure named — the editor's preview becomes a **reviewable artifact**, the chords lead to the change review, and the review is the apply step, through ADR-0008's existing accepted path (no new ADR). The candidate is **carried** in the read result rather than registered: `LspReadGet` registers nothing, no protocol request exports a preview, and the TUI's `app.lsp_tool` registry is `None` in socket/remote mode. The apply path needs no registry either — `apply_preview` recomputes the digest over the request's own patches, so a candidate altered in transit fails the check. **M019 closed 2026-10-20** (`plans/closure/tui-editor-helix-parity/019-status.md`): `o` and `O` were **bound and inert** — the code that opens the line was written, unit-tested, and unreachable from the key path, so both keys dropped the reader into `INS` on the line they were already on, while `docs/tui.md` had been describing the working behaviour the whole time. The two arms now go through the **shared edit funnel**, so the line they open is a real document edit and a read-only attachment refuses them and says so. Two real defects were found by the milestone's own dispatch-path tests and fixed: the open-line cursor was **one byte past** the line it opened, which put the reader's typing on the *next* line; and the open-line edit was being folded into the typing run it cannot continue, which split the word the reader typed across two entries — a typing run now starts only where the cursor ends. The line and the word are **two** undo steps, registered as a residual rather than papered over. The chord and leaf counts did not move: **2 / 71 / 8** and **5 / 44 / 12**. **M020 closed 2026-10-20** (`plans/closure/tui-editor-helix-parity/020-status.md`): the roadmap's §11 binding bullet named a **dead** help overlay (`src/tui/components/help_overlay.rs` is unreferenced), a `:` palette that is a command registry rather than a keymap, and a hand-maintained document that **nothing compared to the code** — so the bullet could not be true, and could not be false either. §11 now carries a dated **Amendment A1** narrowing it to the surfaces that exist, and `scripts/check_tui_editor_doc_drift.py` makes the amended bullet mechanical: every leaf gesture and claimed chord the code owns is documented, and every key the document declares unbound **is** unbound, checked in both directions through the key- and leaf-authority guards' models. It is registered in `scripts/verify.sh quick` and in the CI `verify` job. The chord and leaf counts did not move: **2 / 71 / 8** and **5 / 44 / 12**. |
