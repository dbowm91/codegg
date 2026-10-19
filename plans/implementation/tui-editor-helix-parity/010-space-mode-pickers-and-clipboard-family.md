# TUI editor Helix parity Milestone 010 — Space mode: pickers and the clipboard family

Status: active
Repository baseline: `a8778e87c` (`feat/auto-20261018-12255c45`); W1 was
implemented against the same baseline (uncommitted at the time of writing)

Source roadmap: `plans/subsystems/tui-editor-helix-parity-roadmap.md` §7 M010
Long-term requirements: `plans/000-long-term-specification.md` (IDE-grade
language intelligence); `plans/002-long-term-roadmap.md` (editor capability
sequencing)
Applicable ADRs: `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`
Primary class: capability

## 1. Objective

Complete the space-mode surface: the **pickers** and the **clipboard family**.

Helix's own description of this layer is *"a kludge of mappings, mostly
pickers."* CodeGG currently has eight space leaves, of which **six are
LSP-typed effects** (`k s S d h f`) and **five** of those are LSP *reads*
reads** (delivered by M013) and one is the clipboard yank. This milestone
adds the **file picker** — reclaiming a chord M013 bound to the wrong thing —
and the **clipboard family** Helix groups with it.

| Chord | Helix command | Effect |
|---|---|---|
| `space f` | `file_picker` | open the workspace file browser |
| `space ?` | `command_palette` | open the editor's `:` command palette |
| `space p` | `paste_clipboard_after` | paste the system clipboard after the selection |
| `space P` | `paste_clipboard_before` | paste the system clipboard before the selection |
| `space Y` | `yank_main_selection_to_clipboard` | yank **only** the primary selection |
| `space R` | `replace_selections_with_clipboard` | replace the selection with the clipboard contents |

and binds the remaining Helix space leaves that have **no substrate here** to
a **visible statement of why they cannot run** — the M013 `space F` precedent
— so that no space key is silently dead.

## 2. Why this milestone is ready

- **Every hard dependency is discharged.** The roadmap's M010 entry lists
  *M013 closed (hard, discharged 2026-10-19), M005 closed (hard), M002
  closed (hard)*. All three are closed; M013's closure is at
  `plans/closure/tui-editor-helix-parity/013-status.md`.
- **The registry already records M010 as unblocked**: *"M010 row → **ready —
  both blockers discharged**"*.
- **The surfaces it needs exist.** The workspace file browser
  (`Dialog::Files`, `src/tui/app/modal.rs:330-333`) is implemented and
  openable via `app.open_dialog(Dialog::Files)`
  (`src/tui/commands/panels.rs:636-639`). The clipboard verbs exist
  (`ClipboardVerb`, `src/tui/editor_effect.rs:41-51`) and are already
  driven from Normal mode by `p` / `P` / `R`.
- **The mechanism it needs was built by M013.** The prefix resolver, the
  leaf tables and the one-hop pending-effect pattern are all in place.

## 3. Current implementation evidence

Verified by reading the tree at `a8778e87c` with the M013 work applied.

### 3.1 The space leaves today

`SPACE_LEAVES` (`src/tui/editor.rs:400-478`):

| Glyph | Action | Provenance |
|---|---|---|
| `y` | `PrefixAction::YankSelection` | M004 |
| `k` | `Lsp(Read(Hover))` | M013 |
| `s` | `Lsp(Read(DocumentSymbols))` | M013 |
| `S` | `Lsp(Read(WorkspaceSymbols))` | M013 |
| `d` | `Lsp(OpenDiagnostics)` | M013 |
| `h` | `Lsp(Read(References))` | M013 |
| `f` | `Lsp(Read(DocumentSymbols))` | M013 — **a duplicate of `space s`** |
| `F` | `Lsp(Unavailable("…no multi-document file picker…"))` | M013 |

**`space f` is bound to the wrong thing.** Its own comment says: *"Helix's
file picker. Multi-document attachment is out of scope, so the closest honest
reading is this document's symbols."* That reasoning is now **obsolete**: the
file browser exists (`Dialog::Files`), and the milestone that owns the
pickers is this one. `space f` is a **duplicate of `space s`** today, and
this milestone reclaims it.

**`space F` is a second file picker** in Helix (`file_picker_in_current_directory`).
CodeGG's editor has **one** workspace root, so there is no distinct "current
working directory" to browse. `space F` therefore stays a visible statement —
with the reason **corrected** to say what is actually true.

**A correction to the roadmap's prose.** The roadmap's M010 amendment lists
"`space s`, `space S`" without saying which is which. The live Helix keymap
says `space s` = **document** symbol picker and `space S` = **workspace**
symbol picker. The **code is right**; any roadmap prose that has them the
other way round is wrong and is corrected in the roadmap amendment.

### 3.2 The leaf mechanism

- `EditorPrefix::Space` (`src/tui/editor.rs:97-123`) → `SPACE_LEAVES`.
- `PrefixLeaf { glyph: char, action: PrefixAction }` (`:318-329`) — the glyph
  is **unmodified**; the module knows nothing about key events.
- `PrefixAction` (`:289-315`) has four arms: `Motion(EditorCommand)`,
  `YankSelection`, `Edit(EditorCommand)`, `Lsp(EditorLspLeafEffect)`.
- `leaf_lookup_char` (`src/tui/commands/editor.rs:2900-2906`) accepts only
  `(NONE, Char)` and `(SHIFT, Char)`; a control, an **alt**, or a
  non-character key is **not a leaf**. So `space Alt-c` is not expressible,
  and `Alt`-modified leaves cannot exist in this mechanism.
- `EditorPrefix::swallows_unmatched` (`:276`) is `true` only for `Goto`, so
  an unmatched space key is **retried**, not swallowed.

### 3.3 The one-hop pending-effect pattern (M013's)

The prefix resolver holds `&mut EditorState`; only the App holds a core
client, task registry and toasts. M013 solved that with one hop:

**This is the pre-W1 state. W1 has landed and moved all three:**

- the effect enum is now `EditorPendingEffect`
  (`src/tui/editor_effect.rs:83-113`) with `Read(LspReadOperation)`,
  `OpenDiagnostics`, `DiagnosticStep(EditorDiagnosticStep)`,
  `Clipboard(ClipboardVerb)`, `YankPrimary`, `OpenFilePicker`,
  `OpenCommandPalette` and `Unavailable(&'static str)`;
- the pending slot is now `EditorState::pending_effect`
  (`src/tui/app/state/editor.rs:314`), not `EditorLspState::pending_leaf`;
- `handle_editor_key` drains it on the very next statement
  (`src/tui/commands/editor.rs:2199-2200`), and the runner is
  `run_editor_pending_effect` (`:2238-2271`).

`EditorLspLeafEffect` and `pending_leaf` no longer exist anywhere under
`src/`. The M013 mechanism is otherwise unchanged: the resolver parks the
effect, the App drains it one statement later, and the effect is named
rather than performed.

**The effect enum is LSP-typed, and so is its home.** A file picker, a
command palette and a clipboard verb are **not** LSP facts, and
`EditorLspState` is reset wholesale on a scope change
(`EditorLspState::reset_for_scope`, `:132`). §6.1 generalizes the mechanism.

### 3.4 The picker inventory — what exists and what does not

| Picker | Surface | Reachable from the editor route? | Evidence |
|---|---|---|---|
| **Workspace file browser** | `Dialog::Files` (modal box) | **No** — `/files` is untypeable in the editor route (the editor claims `/` for its search line) | `src/tui/command.rs:601`; `src/tui/app/modal.rs:330` |
| Project picker | `Dialog::ProjectPicker` | Yes, by fall-through (`F`, `Shift+F`, `Ctrl+\`) | `src/tui/app/modal.rs:233-243`; `src/tui/input.rs:660-666` |
| Document-symbol picker | `EditorLspPanel::ReadResult` | **Yes** — `space s` | `src/tui/editor.rs:442-470` |
| Workspace-symbol picker | `EditorLspPanel::ReadResult` | **Yes** — `space S` | `src/tui/editor.rs:448-453` |
| Diagnostic picker | `EditorLspPanel::Diagnostics` | **Yes** — `space d`, `F2` | `src/tui/editor.rs:454-457` |
| Editor command palette | the `:` modal line | **Yes** — `:` | `src/tui/editor.rs:1055-1153` |
| Keymap help | `Dialog::Help` | **Yes**, by fall-through — `?` or `Space` (global Normal) | `src/tui/input.rs:778, 828, 1023, 1055` |
| File-tree pane | side pane | **Yes** — `Ctrl+T` | `src/tui/commands/editor.rs:2047-2050` |
| **Global / workspace search** | — | **does not exist** | `grep -rn "Dialog::…Search" src/tui/` → none; workspace search exists only as model-facing tools (`src/tool/codesearch.rs`, `src/tool/grep.rs`) |
| **Jumplist** | — | **does not exist** | `grep -rn "jumplist\|jump_list\|JumpList" src/` → no hits |
| **Buffer list / buffer picker** | — | **does not exist** | one document at a time; `space F`'s own message says so |
| **Changed-files picker** | — | **does not exist** | `grep -rn "git_service" src/tui/` → 0 matches: no git data source in the TUI at all |
| **Last-picker recall** | — | **does not exist** | — |

### 3.5 The clipboard

- Helpers: `crate::util::clipboard::copy_to_clipboard` (`src/util/clipboard.rs:4`)
  and `read_from_clipboard` (`:19`), both behind the `arboard` feature, which
  is a **default**. Without it, copy errors and read returns `None`.
- `space y` and bare `y` are **the same path** (`src/tui/commands/editor.rs:3480-3487`,
  `:3851-3861`, `:3872-3905`): the selection is turned into byte ranges, the
  selection is **cleared**, and the text is written to the **OS clipboard**
  with a "Copied {n} bytes" toast.
- **There is no editor-local yanked buffer.** The only clipboard call sites
  are the yank and the paste. `docs/tui.md:784-789` says so: *"Paste reads the
  system clipboard, and CodeGG keeps no editor-local register."*
- `enum ClipboardVerb { PasteAfter, PasteBefore, Replace }`
  (`src/tui/editor_effect.rs:41-51`) — **moved there by W1** and now `pub`.
  `ClipboardVerb::build(snapshot, presentation, text) -> Option<EditorEdit>`
  → `editor::paste_after` / `paste_before` /
  `replace_with_text`.
- `run_clipboard_verb(app, verb)` (`src/tui/commands/editor.rs:1712`) →
  `apply_clipboard_text(app, verb, read_from_clipboard())`,
  which distinguishes three failures: read-only buffer / clipboard read
  refused / clipboard empty.
- Normal mode already maps `p` → `BufferKey::PasteAfter` (`:3493`),
  `P` → `PasteBefore` (`:3494-3496`) and `R` → `ReplaceWithClipboard`
  (`:3505`).

### 3.6 The editor command palette

`EditorCommandName::ALL` (`src/tui/editor.rs:1140-1153`) has twelve entries:
`w`, `q`, `wq`, `q!`, `e`, `wa`, `new`, `open <path>`, `complete`,
`split-regex <pattern>`, `keep-regex <pattern>`, `remove-regex <pattern>`.
One list drives the palette, the parser (`parse_editor_command`,
`:1171`) and the help (`:1107-1122`). **There is no picker command in it** —
a picker would be a new `EditorCommandName`, not a new mechanism.

### 3.7 The insert-mode sweep and the route

- The editor route runs **before** the global keymap
  (`src/tui/app/mod.rs:3692-3698`), so an editor leaf wins over a global
  binding.
- `arm_prefix(EditorPrefix::Space)` is called from `handle_normal_key`
  (`src/tui/commands/editor.rs:3547-3550`) — **Normal mode only**. Insert
  mode never arms a prefix, so a space key in Insert mode types a space.
- The insert-mode sweep `printable_characters_reach_the_document_in_insert_mode`
  (`src/tui/commands/editor.rs:5541-5613`) asserts `inspected == 93`
  (`:5609-5612`).

## 4. Invariants that must not regress

1. **One owner per chord.** The key-authority guard
   (`scripts/check_tui_editor_key_authority.py`) attributes every chord to a
   dispatch stage. It does **not** police the prefix leaf tables — a new
   leaf can duplicate a glyph in the same table without the guard noticing.
   §6.6 adds that check.
2. **The leaf lookup accepts only `NONE` and `SHIFT`** (`leaf_lookup_char`).
   A leaf must stay unmodified, and an `Alt`-modified chord must never
   resolve to a leaf.
3. **An unmatched space key is retried, not swallowed**
   (`EditorPrefix::swallows_unmatched` is `true` only for `Goto`).
4. **A prefix never arms in Insert mode**, and a space leaf never steals a
   printable character there. The 93-case sweep is the evidence.
5. **The pending effect is drained on the very next statement**, so a leaf
   can never leave a stale request armed.
6. **A picker that has nothing to show says so.** An empty file browser is
   the M002 failure mode in a different costume.
7. **No LSP write path exists.** `space r` / `space a` / `=` stay unbound
   here; the preview leaves are M015's, and they apply nothing.

## 5. Scope

### In scope

- `space f` → the workspace file browser (**reclaiming** M013's binding).
- `space ?` → the editor's `:` command palette.
- `space p` / `space P` / `space Y` / `space R` — the clipboard family.
- The space leaves with no substrate, bound to a **visible statement**:
  `space F` (one workspace root only), `space b` (one document at a time),
  `space j` (no jumplist), `space D` (no workspace-diagnostics capability),
  `space c` and `space C` (no comment path), `space '` (no fuzzy-picker
  history).
- The **generalized pending-effect mechanism** (§6.1).
- A guard that polices the prefix leaf tables (§6.6).
- The doc surfaces in §12.

### Out of scope, each with a named owner

| Deferred | Why | Owner |
|---|---|---|
| `space r` (rename), `=` (format) | M013 delivered the **previews**; binding them as leaves is a separate milestone so that the space layer's boundary stays "pickers and the clipboard family" | **M015** (new milestone) |
| `space a` (code action) | No code-action **read** exists in the protocol; the preview M013 delivered is for rename and format only | **M015** (blocked on a code-action read) |
| `space g` (changed files) | No git data source in the TUI at all (§3.4) | Deferred, alongside the change-navigation quartet |
| `space /` (global search) | No workspace search in the TUI; workspace search exists only as model-facing tools | A search milestone |
| `space w` / `space W` (window mode) | This editor has no split views | — |
| `space G` (debug) | Helix marks it experimental | — |
| `space Alt-c` (line comment) | **Not expressible** — `PrefixLeaf::glyph` is unmodified and `leaf_lookup_char` accepts only `NONE`/`SHIFT` (§3.2) | A mechanism change, not a binding |
| `space b`, `space j`, `space '` as *working* pickers | The surfaces do not exist; binding them to a working implementation would need a buffer model, a jumplist, and a fuzzy-picker history | Named above, and recorded as deferred product work |
| A **fuzzy** picker | The existing `Dialog::Files` is a tree browser, not a fuzzy matcher | A future picker milestone |
| An **editor-local register** | The roadmap defers named registers; `p`/`P`/`R` read the OS clipboard | The deferred named-registers workstream |

### The divergence this milestone records

**`space f` stops being a document-symbol read and becomes the file picker.**
That is a **breaking change to a live M013 binding**: the same chord now opens
a browser instead of a symbol list. It is recorded in the `CHANGELOG.md` and
in `docs/tui.md`, and the closure states it in terms a reader can act on.

## 6. Required production changes

### 6.1 Generalize the pending-effect mechanism

`EditorLspLeafEffect` and `EditorLspState::pending_leaf` are LSP-typed, and
a file picker is not an LSP fact. The mechanism is generalized, not
duplicated:

- A new module `src/tui/editor_effect.rs` holds
  `EditorPendingEffect`, replacing `EditorLspLeafEffect`:

  ```rust
  pub enum EditorPendingEffect {
      Read(LspReadOperation),        // was EditorLspLeafEffect::Read
      OpenDiagnostics,               // was EditorLspLeafEffect::OpenDiagnostics
      DiagnosticStep(EditorDiagnosticStep),
      Clipboard(ClipboardVerb),      // new
      YankPrimary,                  // new
      OpenFilePicker,               // new
      OpenCommandPalette,           // new
      Unavailable(&'static str),    // was EditorLspLeafEffect::Unavailable
  }
  ```

  `ClipboardVerb` (`PasteAfter`, `PasteBefore`, `Replace`) **moves** here
  from `src/tui/commands/editor.rs:1719-1729` and becomes `pub(crate)`, so
  that the leaf table and the effect enum can both name it.

  > **W1 correction — `ClipboardVerb` is `pub`, not `pub(crate)`.** The
  > compiler rejects the narrower visibility: `EditorPendingEffect` must be
  > `pub` (it is named by the public `PrefixAction::Effect` arm and by
  > `EditorState::pending_effect`), and a `pub` type may not hold a
  > `pub(crate)` one — `private_interfaces` fires, which `clippy -D warnings`
  > would reject. `ClipboardVerb` is therefore `pub`, with the reason
  > recorded in its own doc comment. This is a **compiler-enforced
  > correction**, not a preference.
- The pending slot **moves** from `EditorLspState` to the editor state, as
  `EditorState.pending_effect: Option<EditorPendingEffect>`, beside
  `completion: EditorCompletionState` (`src/tui/app/state/editor.rs:303`).
  It is no longer LSP state, and it is not cleared by an LSP scope change
  for that reason.
- `PrefixAction::Lsp(EditorLspLeafEffect)` becomes
  `PrefixAction::Effect(EditorPendingEffect)`.
- `EditorLspState::reset_for_scope` no longer clears the pending effect (it
  is not LSP state), and `EditorState` gains whatever clearing the slot
  needs on **document** change — a file-picker effect aimed at a document
  the editor no longer shows is stale.

  > **W1 decision — "document change" is `EditorState::detach()`.** That is
  > the one point that unambiguously means "a different document" (switch,
  > close, reload). A local edit does not change *which* document is shown,
  > so it does not clear the slot. The move from "an LSP scope change" to
  > "a document change" is **unobservable**: the effect is parked and drained
  > inside the same `handle_editor_key` call, so it can never be live when a
  > scope change happens.

**This is a rename plus a move**, not a refactor: the M013 mechanism, its
one-hop discipline and its "drained on the next statement" guarantee are
preserved exactly. The M013 **closure record** is not edited — it is
historical evidence — but `architecture/tui.md` and `docs/tui.md` are
updated to the new names, and the closure states that it changed a closed
milestone's surface.

### 6.2 `space f` — the workspace file picker

`space f` → `EditorPendingEffect::OpenFilePicker` → the App calls
`app.open_dialog(Dialog::Files)` (`src/tui/app/modal.rs:330-333`).

- The dialog is **seeded from the shared tree state** today
  (`src/tui/app/modal.rs:332-333`). The milestone must ensure it is seeded
  from the **session's workspace root**, and that the browser **shows which
  root it is browsing**.
- If the session has **no workspace root**, the picker must say so — it
  must not open empty. An empty browser is a key that appears to do
  nothing.
- The picker is a **modal app dialog**, so it takes over the route. On
  dismissal, the editor route is restored exactly as it was.

### 6.3 `space ?` — the command palette

`space ?` → `EditorPendingEffect::OpenCommandPalette` → the App opens the
editor's `:` command line, which is the editor's palette
(`EditorCommandName::ALL`, `src/tui/editor.rs:1140-1153`).

**Today `space ?` works only by accident**: the space prefix's unmatched key
is *retried*, and the global `?` binding then opens the **help** dialog
(`src/tui/input.rs:778, 828, 1023, 1055`). Binding it explicitly replaces
that fall-through with a deliberate behaviour, and distinguishes the
**editor's palette** (`:complete`, `:open <path>`, …) from the **global
help** (which stays on bare `?`).

### 6.4 The clipboard family

| Chord | Effect | Path |
|---|---|---|
| `space p` | paste after the selection | `EditorPendingEffect::Clipboard(ClipboardVerb::PasteAfter)` |
| `space P` | paste before the selection | `…(ClipboardVerb::PasteBefore)` |
| `space R` | replace the selection with the clipboard | `…(ClipboardVerb::Replace)` |
| `space Y` | yank **only** the primary selection | `EditorPendingEffect::YankPrimary` |
| `space y` | yank all selections (unchanged) | existing `PrefixAction::YankSelection` |

- `space p` / `space P` / `space R` reuse `run_clipboard_verb`
  (`src/tui/commands/editor.rs:1712`) and therefore inherit its three
  distinct failures: read-only buffer, clipboard read refused, clipboard
  empty. **No new paste path.**
- `space Y` is **new**: it yanks only the primary member, where `space y`
  yanks every member. It reuses the same clipboard writer and the same
  toast, and it must **clear the selection** exactly as `space y` does — a
  yank that leaves the selection behind is a different gesture.

**`space p` and `space P` duplicate Normal mode's `p` and `P`.** Helix
binds both spellings for the same action, so this is parity rather than a
new capability; the space table is where a reader looks for the clipboard
family, and a table that lists `y` but not `p` is not a clipboard family.

### 6.5 The leaves with no substrate

Each of these becomes `EditorPendingEffect::Unavailable(reason)` with a
**reason a reader can act on**, and the reason is the same text the docs
carry:

| Chord | Reason (the substance) |
|---|---|
| `space F` | the editor has **one** workspace root; `space f` opens it. *(The current message says "no multi-document file picker", which is not why.)* |
| `space b` | only one document is attached at a time; there is no buffer list |
| `space j` | this editor keeps no jumplist |
| `space D` | no server capability backs a separate workspace-diagnostics request; `space d` opens the panel that exists |
| `space c` | no comment path exists in this editor — neither an LSP comment action nor a local one |
| `space C` | same as `space c` (block comments) |
| `space '` | this editor's pickers are not fuzzy, so there is no "last picker" to recall |

`Unavailable` renders as a **status line**, not a toast — the M013
precedent (`EditorLspLeafEffect::Unavailable` renders its own line, and
`NotReady` / `Ambiguous` / `Unsupported` each render their own). A toast
disappears; a status line stays and can be read.

### 6.6 A guard for the leaf tables

The key-authority guard scopes itself to `editor_buffer_key`,
`handle_normal_key` and `handle_insert_key`. **The prefix leaf tables are
not policed at all.** A new leaf can therefore:

- duplicate a glyph inside the same table (the earlier row silently wins),
- claim a chord another table already claims, and
- be misspelled so that the key is dead on every terminal.

This milestone adds a check — either a new
`scripts/check_tui_editor_leaf_authority.py` or an extension of the existing
guard — that, for every prefix table:

1. **no duplicate glyph within a table**;
2. **every glyph is a printable ASCII character**;
3. **every glyph is unique across the tables of the same prefix** (a table's
   rows are its own namespace, so this is about the *same* table);
4. **no leaf glyph is a guard-block chord's glyph** (the guard block runs
   before any mode dispatch, so a leaf that claimed one would be dead).

> **The plan's earlier rule 4 was wrong and is replaced.** It asserted
> that a table's glyph set is *disjoint from the Normal-mode chord set*.
> That is **demonstrably false on the shipped tree**: seven of the eight
> live `SPACE_LEAVES` glyphs are also Normal-mode chords — `k`, `h`, `f`,
> `F`, `S`, `d`, `y` — and only `s` is free. M010's own proposed leaves
> `p`, `P` and `R` are Normal-mode chords too.
>
> The rationale was wrong as well. A leaf is consulted **only while its
> prefix is armed**, and the resolver disarms before consulting it
> (`src/tui/commands/editor.rs:2893+`, lookup at `:2950`), so a leaf
> **cannot** shadow a directly-bound chord. Rules 1–3 plus the
> guard-block rule above are the real invariants.

**Non-vacuity is required**: the closure must show the check **failing** on
an injected duplicate glyph, by injection.

### 6.7 Insert-mode safety

The space prefix is armed only from `handle_normal_key`, so a space key in
Insert mode types a space. The 93-case sweep already proves the printable
path; the milestone adds an explicit test that **`space` + a leaf glyph in
Insert mode types both characters and opens nothing**.

## 7. Ordered work packages

- **W1 — The mechanism. LANDED.** Generalize the pending effect (§6.1): the
  new module, the enum, the slot's new home, the `PrefixAction` arm, and the
  `ClipboardVerb` move. **Nothing binds yet** — the existing eight leaves
  must keep working through the new mechanism, and their tests must pass
  unmodified. That is the proof the generalization was behaviour-preserving.

  W1 delivered `src/tui/editor_effect.rs` holding `EditorPendingEffect`
  (replacing `EditorLspLeafEffect`) and the moved `ClipboardVerb`; the slot
  moved to `EditorState::pending_effect` in `src/tui/app/state/editor.rs`;
  `PrefixAction::Lsp` became `PrefixAction::Effect`; and
  `run_editor_lsp_leaf` became `run_editor_pending_effect`. The rename left
  **no** `EditorLspLeafEffect` or `pending_leaf` reference anywhere under
  `src/`. The 14 M013 leaf/dispatch/stepping tests pass by name, and the
  three editor guards pass. Three of those tests named the old type and were
  retyped **without renaming the test** — the behavioural assertions are
  unchanged, which is the evidence that W1 changed no behaviour.
- **W2 — The file picker.** `space f` (§6.2), including the workspace-root
  seeding and the empty case.
- **W3 — The clipboard family.** `space p` / `P` / `Y` / `R` (§6.4).
- **W4 — The command palette.** `space ?` (§6.3).
- **W5 — The visible statements.** The seven `Unavailable` leaves (§6.5),
  including the corrected `space F` reason.
- **W6 — The guard.** The leaf-table check (§6.6) and its non-vacuity proof.
- **W7 — Docs and the sweep.** The doc surfaces in §12; the `CHANGELOG.md`
  entry, including the `space f` breaking change.

## 8. Failure, cancellation, restart, contention semantics

- **The file picker is a modal app dialog.** It takes over the route; on
  dismissal the editor route is restored. A dismissed picker leaves the
  document, the selection and the undo history untouched.
- **The clipboard failures keep their three distinct messages** (read-only
  buffer / read refused / empty). They are not collapsed into "paste
  failed".
- **`space Y` on an empty selection** must say "Nothing to yank" — the same
  message `space y` uses — not write an empty string to the clipboard.
- **The pending effect is drained on the very next statement**, so a leaf
  can never leave a stale request armed. A document change clears it.
- **The picker is not a daemon round trip.** It reads the tree state the App
  already holds. No protocol, no authorization, no audit entry.
- **No concurrency surface.** The space prefix is single-threaded, in the
  App's key handler.

## 9. Compatibility and migration

- **No protocol change.** `crates/codegg-protocol` is untouched.
- **No storage change.** No migration, no schema bump.
- **No daemon change.**
- **One user-visible behaviour change**: `space f` opens the file browser
  instead of a document-symbol read. It is recorded in the
  `CHANGELOG.md` under `### Changed`, not `### Added`, because it displaces a
  live binding.
- **`EditorLspLeafEffect` → `EditorPendingEffect`**, and the pending slot
  moves from `EditorLspState` to `EditorState`. Both are internal to the
  TUI crate; the `codegg-client` crate and the WS protocol are unaffected.
- **The insert-mode sweep stays at 93.**
- **The global keymap is not changed.** `src/tui/input.rs` keeps its
  bindings; the editor route claims the space prefix first.

## 10. Required tests

Every leaf is driven through the **real dispatch path** — `App::on_key` →
`handle_editor_key` → `editor_buffer_key` → `handle_normal_key` → the
prefix resolver — not through a helper.

- **The reclaim**:
  - `space f` opens the file browser, and the browser names the workspace
    root it is browsing;
  - `space f` with **no workspace root** says so and opens nothing;
  - `space s` still opens the **document**-symbol read (the two are no
    longer the same).
- **The clipboard family**:
  - `space p` pastes after the selection, `space P` before it — each
    asserting the resulting text;
  - `space R` replaces the selection with the clipboard contents;
  - `space Y` yanks **only** the primary member of a multi-member selection,
    and the clipboard holds just that member's text;
  - `space Y` on an empty selection says "Nothing to yank";
  - a paste into a **read-only** buffer is refused with the read-only
    reason;
  - an **empty** clipboard pastes nothing and says so.
- **The visible statements**: each of the seven `Unavailable` leaves renders
  its reason, and the reason is the one the docs carry.
- **`space ?`** opens the editor's `:` command palette, and the palette
  lists the twelve `EditorCommandName` entries.
- **Insert-mode safety**: `space` then a leaf glyph in Insert mode **types
  both characters** and opens nothing; the 93-case sweep is unmodified.
- **Keymap consistency**: the same `space f` gesture behaves identically
  with the global keymap in its default and vim configurations
  (`build_bindings(None, vim_mode)`), because the space prefix is
  editor-local.
- **The guard's non-vacuity**: an injected duplicate glyph in a leaf table
  makes the new check fail. Prove it by injection before citing the check
  as evidence.
- **Regression**: the existing M013 leaf tests — the fourteen dispatch and
  stepping tests in `src/tui/commands/editor.rs` — must pass **unmodified**
  after the mechanism is generalized. That is the evidence that W1
  changed no behaviour.

## 11. Required verification commands

```bash
cd /Users/davidbowman/projects/codegg/.worktrees/feat-auto-20261018-12255c45

cargo fmt && cargo fmt --check
CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=2 cargo build --locked --bin codegg-sandbox-helper
CARGO_BUILD_JOBS=2 cargo test -p codegg --lib --no-fail-fast

python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_tui_editor_prefix_state.py
python3 scripts/check_tui_editor_text_authority.py
bash scripts/verify.sh quick
```

`verify.sh full` is the wider sweep; the pre-existing 69 failing `tests/`
integration binaries are **not** this milestone's and are not fixed here (see
`plans/registry.md`, "Residual registered, not discharged by M013"). A
closure that claims a green full sweep must have one.

## 12. Documentation updates

| File | Section | What changes |
|---|---|---|
| `docs/tui.md` | the space-mode table | the full Helix space surface: what is bound, what is a visible statement, and what is out of scope |
| `docs/tui.md` | `### Multi-key gestures` (`:845`) | the space prefix's second level |
| `docs/tui.md` | the clipboard prose (`:784-789`) | `space Y` and the space-mode clipboard family |
| `architecture/tui.md` | the pending-effect / one-hop section | the generalized mechanism and its new home |
| `architecture/tui.md` | the editor keymap and its degradation contract | the space-mode leaves and the `space f` change |
| `architecture/tui.md` | the module map (`:4515-4519` area) | the new `src/tui/editor_effect.rs` module |
| `CHANGELOG.md` | `## [Unreleased]` → `### Added` / `### Changed` | the picker, the clipboard family, and the `space f` change under `### Changed` |
| `.opencode/skills/tui/SKILL.md` | the editor-grammar bullet | the space surface and the leaf-table guard |

`AGENTS.md` requires the skill and the `architecture/` doc to move together.

`src/tui/components/help_overlay.rs` is **unreferenced dead code** and is
not a doc surface; it is not edited.

## 13. Acceptance criteria (externally observable)

1. `space f` opens the workspace file browser, which names the root it is
   browsing; with no root it says so.
2. `space s` still opens the document-symbol read, and the two are no
   longer the same gesture.
3. `space ?` opens the editor's `:` command palette.
4. `space p`, `space P`, `space R` and `space Y` do what Helix's do, with
   the clipboard's three distinct failure messages preserved.
5. `space Y` yanks only the primary member of a multi-member selection.
6. Each of the seven substrate-less leaves renders a reason, and the reason
   is the one the docs carry.
7. No space key is silently dead: every Helix space leaf is either bound,
   bound to a visible statement, or documented as out of scope with its
   reason.
8. In Insert mode, `space` and every leaf glyph type text; nothing opens.
9. The same `space` gesture behaves identically under the default and vim
   global keymaps.
10. The leaf-table check fails on an injected duplicate glyph.
11. The fourteen M013 leaf tests pass **unmodified**, and the insert-mode
    sweep still asserts `inspected == 93`.
12. `scripts/verify.sh quick` passes.

## 14. Stop conditions

Stop and report rather than continue if any of these holds.

- **Generalizing the pending effect requires a mechanism change rather than
  a rename and a move.** Two pending slots would be two mechanisms, and one
  slot that is not LSP state must not live in LSP state — but a deeper
  restructuring is a design change, not a binding change.
- **`Dialog::Files` cannot be seeded with the session's workspace root.**
  The alternative is a browser that shows the wrong tree or an empty one.
- **A space leaf would have to shadow a directly-bound Normal-mode chord.**
  The leaf-table check (§6.6) exists to catch exactly this.
- **The clipboard family needs an editor-local register.** The roadmap
  defers registers; `p`/`P`/`R` read the OS clipboard today, and this
  milestone must not change that contract.
- **A leaf needs an `Alt` modifier.** `PrefixLeaf::glyph` is unmodified and
  `leaf_lookup_char` accepts only `NONE`/`SHIFT`; a modified leaf is a
  mechanism change with its own milestone.
- **A picker is proposed that needs a daemon round trip.** The file browser
  reads tree state the App already holds; a picker that needs new authority
  is a different milestone with a different authorization story.

## 15. Closure evidence required

- A closure record at
  `plans/closure/tui-editor-helix-parity/010-status.md` with: the baseline
  SHA, the work packages landed, the guard outputs, the test names and
  counts for every suite in §11, and the exit conditions with their
  evidence.
- **The `space f` change stated as a change**, not an addition: what the
  chord did before, what it does now, and where the `CHANGELOG.md` says so.
- The **generalization evidence**: the fourteen M013 leaf tests passing
  unmodified, with their names.
- The **leaf-table check's non-vacuity proof**, with the command and the
  observed failure.
- The **seven reasons**, each with its doc reference.
- An explicit statement of what M015 now owns (`space r`, `=`, `space a`)
  and why it was separated rather than folded in.

## 16. Handoff notes

- **The interesting part of this milestone is one wrong binding.**
  `space f` is bound to a *duplicate* of `space s`, and M013's own comment
  explains why: at the time, the file browser was not reachable from the
  editor route. The browser exists (`Dialog::Files`); the route was the
  problem; this milestone is the route. A reviewer should check §3.1 and
  §6.2 rather than assume the reclaim is a rename.
- **The "visible statement" pattern is M013's, and it is the right one.** A
  key that says why it cannot run is a different thing from a key that does
  nothing. Seven of the space leaves land in that category, which says more
  about this editor's surface than a list of what works.
- **The mechanism generalization is small and must stay small.** W1 changes
  names and one field's home. If it starts changing behaviour, the
  milestone has outgrown its boundary.
- **The space layer is a kludge in Helix too.** The goal is not a beautiful
  menu; it is that every chord a reader tries has an answer.
