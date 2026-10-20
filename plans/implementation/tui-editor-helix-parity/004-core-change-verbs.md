# TUI editor Helix parity Milestone 004 — Core change verbs (paste, bare `y`, `r`/`R`, `~`)

Status: closed — see `plans/closure/tui-editor-helix-parity/004-status.md`
and amendment A1 (§17) below, which corrects §1, §5, §6, §8 and §13 where
they disagree with the live Helix keymap.
Repository baseline: `84440fe7f` (`feat/auto-20261018-12255c45`)

Source roadmap: `plans/subsystems/tui-editor-helix-parity-roadmap.md` §7 M004
Long-term requirements: `plans/000-long-term-specification.md` (TUI editor);
`plans/002-long-term-roadmap.md` (editor capability sequencing)
Applicable ADRs: `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`
(editor-owned text state; no app-side LSP startup)
Primary class: capability

## 1. Objective

Close the missing basic edit loop by landing the four Helix change verbs that
need no new infrastructure beyond the existing edit funnel:

- `p` / `P` — paste after / before the cursor.
- bare `y` — yank the current selection to the system clipboard.
- `r` / `R` — replace one character / replace until end of line.
- `~` and its case companions — toggle the case of the character under the
  cursor or of the held selection.

Today `space y` yanks a selection and nothing pastes it, so a user cannot move
text at all without leaving the editor. This milestone makes `y` followed by `p`
a complete round trip.

## 2. Why this milestone is ready

- **M003 — closed** (`plans/closure/tui-editor-helix-parity/003-status.md`). The
  shared motion/verb resolution path and the `EditorPrefix` grammar are in
  place, so new verbs are additive arms rather than a new mechanism.
- **M002 — closed.** `resolve_pending_prefix` resolves multi-key gestures; no
  new prefix state is required for this milestone.
- No unresolved ADR gates this scope. Every verb here is a pure text
  transformation against the frontend document replica.

Interface dependency considered and confirmed stable: `EditorEdit` /
`submit_edit` (see §4). M012's undo-traversal change will extend the history
model but is not required for these verbs to land first.

## 3. Current implementation evidence

### Yank exists and is clipboard-only

`handle_normal_key` (`src/tui/commands/editor.rs`) routes `space y` to a
selection-yank arm that calls `yank_editor_range`
(`src/tui/commands/editor.rs`). That helper:

1. reads the range with `snapshot.read_bytes(range.start, range.end)`,
2. calls `crate::util::clipboard::copy_to_clipboard`,
3. reports success/failure through `app.messages_state.toasts`.

`crate::util::clipboard` already exposes **both** directions:

- `copy_to_clipboard(&str) -> Result<(), AppError>` — `arboard::set_text`.
- `read_from_clipboard() -> Option<String>` — `arboard::get_text`.

Both are feature-gated on `arboard`, which is a **default** feature
(`Cargo.toml`), and both have a non-`arboard` stub that returns
`Err(AppError::Clipboard("clipboard support not enabled"))` /
`None`. Paste therefore already has a source and a typed failure mode.

**There is no editor-local register.** `yank_editor_range` writes only to the
system clipboard; nothing in `TuiDocumentPresentation` stores yanked text. So
`p` must read from `read_from_clipboard()`, and every paste is subject to
whatever is on the OS clipboard — including text yanked from another
application. This is a real, accepted consequence of Helix's own clipboard
integration, and it is stated here rather than discovered later.

### The edit funnel

`submit_edit` (`src/tui/commands/editor.rs`) is the single mutation funnel. It:

- calls `session.presentation_mut().cancel_search()` — a `/` search's byte
  offsets describe pre-edit text and are dropped here rather than per-verb,
- applies `controller.apply_local(transaction)`,
- clamps the cursor against the **post**-edit snapshot,
- pushes the undo entry **only after** the controller accepts, so a rejected
  edit leaves no phantom inverse,
- clears LSP semantic tokens, arms the semantic refresh, clears completion,
  marks diagnostics stale, and sets `EditorStatus::DirtyLocal`.

`editor_editing_command` gates on `editable(state)`:
`state.is_open() && !state.is_busy() && state.is_writer() && !state.status.blocks_edits()`.
Every verb in this milestone MUST route through it.

### What does not exist

There is **no paste binding of any kind**, no `r`/`R`, no `~`, and no
character-case transform. Confirmed by the Normal-mode dispatch table and by
`EditorCommandName`'s command set.

## 4. Invariants that must not regress

1. **Single mutation funnel.** Every verb produces an `EditorEdit` and goes
   through `submit_edit` → `apply_local`. No verb writes text directly.
2. **Unclaimed-key fallthrough.** An unclaimed Normal-mode key still returns
   `BufferKey::Unhandled` so it reaches the composer. Adding `p`, `P`, `y`,
   `r`, `R`, `~` **narrows the composer keyspace in Normal mode only** —
   intended, but the contract must still hold for everything else.
3. **Insert-mode text integrity.** None of these keys may be claimed while the
   route is in Insert. `p`, `y`, `r`, `R`, `~` must keep inserting literal
   text there.
4. **Char boundary safety.** No transform may split a multi-byte character.
   This is a live hazard in this codebase: M003 found that `read_bytes`
   refuses a range that is not boundary-aligned on **both** ends, so any
   transform built from `read_bytes(offset, offset+1)` cannot read the leading
   byte of a multi-byte character.
5. **Undo correctness.** A verb that changes text must be undoable as one
   entry, with the inverse recorded only on acceptance.
6. **Search cancellation** and LSP staleness continue to flow through
   `submit_edit` — no verb bypasses it.
7. **Read-only refusal.** All verbs refuse on a read-only attachment with the
   existing notice, not a silent no-op.

## 5. Scope

### In scope

- `p` paste after cursor; `P` paste before cursor.
- bare `y` yank the held selection.
- `r` replace the character under the cursor with a captured character.
- `R` replace from the cursor to end of line with captured text (Helix: enters
  Insert mode seeded with the retained tail removed).
- `~` toggle case of the character under the cursor, or of the whole held
  selection when one exists.
- Case verbs for the held selection to the extent `~` needs them internally;
  Helix's full `Alt-c` family is **out of scope** (see below).
- Tests, guard updates, and documentation updates.

### Out of scope

- `c` / `Alt-c` (change-and-insert) — M011 chord territory, requires insert-mode
  control work.
- `noyank` variants (`d`, `c`, `y` with a trailing `n`).
- `>` / `<` indent and dedent.
- `=` format — **requires an LSP protocol extension**; see
  `plans/subsystems/tui-editor-helix-parity-roadmap.md` §10 risk 2.
- Any **LSP** work whatsoever in this milestone. The verbs here are pure text
  transformations and must not acquire an LSP dependency. This is deliberate:
  LSP reachability is owned by M013/M010 and is blocked on a separate
  prerequisite (see `plans/implementation/tui-editor-helix-parity/013-lsp-read-delivery-and-reachability.md`).
- Register support (`"<reg>`, `space` register pickers).
- `J`, `.` repeat, backtick/`Ctrl-a` increment — all deferred product work.

## 6. Required production changes

### Presentation / state

- A character-capture state for `r` and `R`, alongside the existing
  find-target capture that M003 introduced. **Prefer extending that mechanism
  rather than adding a second one** — M003 established the rule that a prefix
  whose next key is *data* has `leaf() == None` and its own capture. If a new
  `EditorPrefix` variant is used, `scripts/check_tui_editor_prefix_state.py`
  must still pass, and `leaf()` must return `None` for it.
- For `R`, the retained tail is held for the duration of the Insert session and
  must be **discarded** when Insert is exited without typing, and must not
  survive a document reload or a buffer switch.

### Editor / text layer

Implement in `src/tui/editor.rs` (pure, snapshot-driven, no UI):

- `paste_after(snapshot, cursor, text) -> Option<EditorEdit>`
- `paste_before(snapshot, cursor, text) -> Option<EditorEdit>`
- `replace_char(snapshot, cursor, replacement) -> Option<EditorEdit>`
- `replace_to_line_end(snapshot, cursor) -> Option<EditorEdit>` (produces the
  `stay_insert` variant with the tail removed)
- `toggle_case_at(snapshot, cursor) -> Option<EditorEdit>`
- `toggle_case_range(snapshot, range) -> Option<EditorEdit>`

Each returns `None` when the operation is a no-op for the given snapshot (empty
clipboard, cursor at end of buffer for `p`, cursor past the last character for
`r`, a character already in both cases is impossible but an empty selection
range is not), so the caller can distinguish "did nothing" from "refused".

`toggle_case_*` must use Unicode-aware case mapping (`char::to_uppercase` /
`to_lowercase`), and must **expand** when the mapped form differs in length
(e.g. `ß` → `SS`). An edit whose insert text is longer than the replaced range
is fine — `EditorEdit` already carries arbitrary transaction/inverse pairs —
but the **cursor clamp** in `submit_edit` runs against the post-edit snapshot
precisely so this is safe.

### Dispatch

In `src/tui/commands/editor.rs`:

- Add arms for `p`, `P`, `y`, `r`, `R`, `~` in `handle_normal_key`, each
  gated on `editable(state)` and routing to `editor_editing_command` /
  `submit_edit`.
- `r` and `R` arm the character-capture prefix.
- Paste reads `crate::util::clipboard::read_from_clipboard()` and reports
  failure through the existing toast surface. Do **not** invent a second
  clipboard abstraction.

### Guards

- `scripts/check_tui_editor_key_authority.py` attributes every chord to its
  dispatch stage. New bindings must be attributed to the Normal-mode editor
  stage, and must not re-declare a chord an earlier stage owns.
- If a capture prefix variant is added, `scripts/check_tui_editor_prefix_state.py`
  must be updated to know about it.

### Documentation

- `docs/tui.md` — the editor keymap section gains the six new bindings.
- `architecture/tui.md` — record that the editor has no local register and that
  paste is clipboard-sourced, since that is a contract a future maintainer will
  otherwise assume is otherwise.

## 7. Ordered work packages

**W1 — Pure text transforms + unit tests.** Implement the six functions in
`src/tui/editor.rs` with tests covering: multi-byte characters at buffer
boundaries, case-mapping length expansion, empty clipboard text, cursor at
document end, empty selection range, and CRLF line endings for `R`.
Acceptance: `cargo test -p codegg --lib tui::editor` green.

**W2 — Dispatch arms + integration tests.** Add the six Normal-mode arms and
capture behaviour. Tests must drive keys through `editor_buffer_key` and assert
the observable document change and the reported `BufferKey`, per the M001
convention. Include the negative cases: read-only attachment refuses; Insert
mode still inserts `p`, `y`, `r`, `R`, `~` as literal text.
Acceptance: focused editor dispatch tests green.

**W3 — Guards and docs.** Run the key-authority and prefix-state guards, update
`docs/tui.md` and `architecture/tui.md`.
Acceptance: `scripts/check_tui_editor_key_authority.py` and
`scripts/check_tui_editor_prefix_state.py` both exit 0.

**W4 — Full verification.** §11.

## 8. Failure, cancellation, restart, contention semantics

- A clipboard read that fails is a **toast**, not a crash and not a silent
  no-op — this is the same reasoning the existing yank helper documents.
- An empty clipboard string is a no-op, distinguishable from a read failure.
- `apply_local` rejection (destructive phase in flight, full queue) is reported
  through the existing notice path; the editor never retries internally.
- `R` cancelled with `Esc` before any typing restores the tail: the tail was
  never removed until the first accepted edit, or is removed by an undoable
  edit that `Esc` then discards. Whichever is chosen, it MUST be covered by a
  test.
- The retained `R` tail MUST NOT survive a buffer switch or document reload.

## 9. Compatibility and migration

None. No protocol, storage, or configuration change. Bindings are additive and
are claimed only in Normal mode. Existing `space y` keeps its current
behaviour, including its toast feedback; bare `y` is a **new** separate arm and
must not silently change what `space y` reports.

## 10. Required tests

- Focused unit tests in `src/tui/editor.rs` for each transform, including the
  multi-byte and case-expansion cases named in W1.
- Dispatch tests through `editor_buffer_key` asserting both `BufferKey` and
  observable change (M001 convention).
- Insert-mode integrity tests: each new key inserts literally.
- Read-only refusal test per verb.
- `R` cancellation test.
- Guard coverage: the key-authority script must fail on an injected violation
  before being cited as evidence (per the `check_daemon_cwd_usage.py` precedent
  in `AGENTS.md`).

## 11. Required verification commands

```bash
cargo fmt
cargo test -p codegg --lib tui::editor
cargo test --test tui_editor_dispatch   # narrowest covering integration target
python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_tui_editor_prefix_state.py
scripts/verify.sh quick
```

`verify.sh full` is required before closure.

## 12. Documentation updates

- `docs/tui.md` (editor keymap table).
- `architecture/tui.md` (no local register; clipboard-sourced paste).
- `CHANGELOG.md` under the unreleased section, if this repo's convention
  requires it for user-visible keys — verify against recent entries.

## 13. Acceptance criteria (externally observable)

1. With text on the clipboard, `y` on a selection then `p` pastes it; `P`
   pastes before the cursor.
2. `r` followed by a printable character replaces exactly one character under
   the cursor, leaving the rest of the line intact.
3. `R` followed by typing replaces to end of line; `Esc` cancels without
   corrupting the line.
4. `~` toggles the case under the cursor, and toggles every character of a held
   selection when one exists.
5. None of these keys are captured in Insert mode.
6. A read-only attachment refuses each verb visibly.
7. A multi-byte character under `r`/`~`/`R` is never split.
8. `space y` behaves exactly as before.

## 14. Stop conditions

Report rather than improvise if:

- A transform cannot be expressed as a single `EditorEdit` through
  `apply_local` without a new text-authority path.
- `R` retention cannot be made to survive neither buffer switch nor reload
  without a new presentation field the guard rejects.
- Implementing paste reveals that the clipboard source is unavailable in a
  supported configuration such that `p` can never work — report before adding a
  fallback register, which is a scope decision, not a mechanics one.
- Any change appears to require relaxing ADR-0011.

## 15. Closure evidence required

Implementation commits; the §11 command output with results labelled local vs
CI; an explicit statement for each of `c`/`Alt-c`, noyank, `>`/`<`, and `=`
that it was **deferred by scope** rather than forgotten; and a guard
demonstration that `check_tui_editor_key_authority.py` fails on an injected
violation.

## 16. Handoff notes

Start at W1; the transforms are independent of dispatch and can be reviewed on
their own. The one judgement call worth surfacing early is the `R` cancellation
semantics in §8 — decide it before W2 so the test can be written against a
chosen behaviour rather than discovered afterwards.

The nearest precedent to copy structure from is
`plans/implementation/tui-editor-helix-parity/003-character-finding-motions.md`,
which documents the same funnel and the same char-boundary hazard.
---

## 17. Amendment A1 (implementation-time): `R` is `replace_with_yanked`, not "replace to end of line"

Recorded after implementation, before closure. The plan above is unamended
above this line so the original reasoning stays auditable; this section
supersedes it where the two disagree, because the plan was wrong and the
normative source is not.

### What the plan got wrong

§1, §5, §6 and §13 all say `R` replaces "to end of line" with captured text,
entering Insert mode with the retained tail removed. That is vi's `R`, and it
was an older Helix binding. The current
[Helix keymap](https://docs.helix-editor.com/keymap.html) — re-read at the
keystroke, which is what M005's `gl`/`gt` correction established as the
method — reads:

> `R` | Replace with yanked text

So `R` takes **no** argument, enters **no** Insert mode, and retains **no**
tail. Everything in §8 about a "retained R tail" — the presentation field, the
`Esc` restore hook, the cancellation test, the "does not survive a buffer
switch" requirement — described a design for a verb that does not exist.

The plan's own parenthetical in §5 ("Helix: enters Insert mode seeded with the
retained tail removed") contradicted the adjacent phrase "with captured text".
The inconsistency was in the source document, not introduced here.

### What changed in the implementation

| Plan said | Implementation | Why |
|---|---|---|
| `R` arms a capture prefix | `R` is a single key | It takes no argument |
| `R` enters Insert mode | It stays in Normal | It is a replacement, not a mode change |
| `R` retains a tail on the presentation | It retains nothing | Helix retains nothing |
| `~` needs no `EditorPrefix` variant | `r` adds `EditorPrefix::ReplaceChar` | `r` *does* take an argument |

The `R` design collapses to the same shape as `space y`: the buffer reports a
verb, the App layer reads the clipboard, the transform replaces the selection
(or the character under the cursor). It is `ClipboardVerb::Replace`.

This also **removed the milestone's one judgement call**. §8 flagged the tail
retention as worth surfacing early and §14 listed it as a stop condition. Both
are moot; the question cannot arise for a verb that retains nothing.

### The consequence that had to be paid: `R` was already bound

`R` reload the buffer from disk, as a Normal-only **guard-block** chord in
`editor_buffer_key` — a stage answered *before* mode dispatch, and therefore
capable of both shadowing a Normal-mode verb and surviving into Insert.

`scripts/check_tui_editor_key_authority.py` fails on exactly that
combination (proved by injection — see the closure record), so the rebinding
forced the chord out. `BufferKey::RequestReload` and its only producer are
gone, and the guard's `REQUIRED_SUBSTRINGS` moved from three guard-block
chords to two.

**Reload survives as `:e`.** `EditorCommandName::Reload` already reached
`reload_editor` directly, so no capability was lost — only a keystroke
moved. That is a user-visible change and is called out in `docs/tui.md`, in
`architecture/tui.md`, and in the help overlay.

### Acceptance criteria amended

§13 criterion 3 ("`R` followed by typing replaces to end of line; `Esc`
cancels without corrupting the line") is **withdrawn** — it tested a verb
Helix does not have. Replaced by: "`R` replaces the held selection, or the
character under the cursor, with the system clipboard's contents."

§13 criterion 6 ("`R` retention cannot be made to survive neither buffer
switch nor reload without a new presentation field the guard rejects") is
**withdrawn** for the same reason. Nothing is retained, so the text-authority
guard is not asked to arbitrate a field that does not exist.

Criteria 1, 2, 4, 5, 7 and 8 stand as written.

### Not added, deliberately

`space p` / `space P` (Helix's `paste_clipboard_after` / `_before`) are the
same three transforms through the space-mode leaf table. They are one row each
in `SPACE_LEAVES` and are left to the space-mode milestone that owns that
table, so this milestone does not quietly widen the table it was given.
