# TUI Editor Helix Capability Parity Milestone 007 — Registers

Status: conditionally closed
Repository baseline: `7642b9e381d9c91d650fa375be2a7aa0aae9bcd8`

Source roadmap:

- `plans/subsystems/tui-editor-helix-capability-parity-roadmap.md#milestone-007--registers`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#29-system-invariants`

Applicable ADRs:

- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`

Primary class: capability

## 1. Objective

Introduce named registers so `"<reg>` selects where a yank or a delete goes and
where a paste comes from, with the unnamed register, the blackhole register and
the search register, and with the system clipboard as an explicitly-spelled
sibling rather than the only target.

## 2. Why this milestone is ready

No hard dependency. It discharges an obligation another milestone created.

- Roadmap M002's **D1** explicitly deferred yank-on-change to this milestone,
  because with no register the only yank target was the system clipboard and
  clobbering it on every delete is a user-hostile regression. M002 bound
  `Alt-c`/`Alt-d` as the no-yank spellings and recorded that this milestone makes
  `c`/`d` yank. **That is this milestone's contract.**
- The yank path exists and is one function: `yank_editor_ranges` reads the
  selected ranges into a local `String` and hands it to the clipboard. A register
  write is the same read, with a different destination.
- The paste path exists and is three verbs behind one `ClipboardVerb` enum
  (`PasteAfter`, `PasteBefore`, `Replace`), each building an `EditorEdit` from
  the text. A register paste is the same builder with different bytes.
- The prefix machine already carries a captured argument one level deep — the
  `m` grammar's `MatchArg` — so `"` plus a name is the same shape as `m` plus a
  command plus an argument.
- The text-authority guard's rules are narrow and known (§7 work package A
  states them), so a register store can be shaped to satisfy them rather than
  discovered to violate them.

## 3. Current implementation evidence

At the baseline:

- `y` and `space y` both resolve to `BufferKey::YankRanges(ranges)`, and
  `yank_editor_ranges` copies the bytes to the **system clipboard** via
  `crate::util::clipboard::copy_to_clipboard`. It reads each range through
  `snapshot.read_bytes` into a local `String` and skips an unreadable member
  rather than losing the whole verb.
- `p`, `P` and `R` resolve to `BufferKey::PasteAfter/PasteBefore/ReplaceWithClipboard`
  carrying a `ClipboardVerb`; the App layer reads the clipboard and calls the
  verb's `build`.
- **There is no register of any kind.** `"` is unbound in Normal mode, so it
  falls through to the composer. The predecessor's M011 documentation records
  that `Ctrl-r` insert-register was deferred precisely because there is no
  substrate.
- **`Ctrl-r` is the guard block's resync chord**, mode-independent, and the key
  authority guard asserts it by name. It runs before any mode dispatch.
- `d` does not write the clipboard; `Alt-d` and `Alt-c` were bound in M002 as the
  explicit no-yank spellings.
- The text-authority guard treats a **field named** `text`, `content`, `lines`,
  `body`, `buffer`, `rope`, `source` or `document` as a violation, and a field
  holding `Vec<String>` or `Vec<Line>` as a violation, and an owned
  `DocumentSnapshot`/`DocumentBuffer`/`Rope` as a violation. A local `String` is
  fine — `yank_editor_ranges` already has one.
- The key authority guard reports **2 / 74 / 8** with no shadowed arm.

## 4. Invariants that must not regress

- **A register is bounded.** A named byte cap; reaching it is disclosed, not
  silently truncated.
- **Registers are frontend-local and lifecycle-cleared** on open, reload, resync,
  reconnect and close — the same contract as the undo history.
- **`Alt-c` and `Alt-d` never touch a register and never touch the clipboard.**
  This is M002's promise and it must survive the milestone that makes `c`/`d`
  yank.
- **The `space` clipboard family always means the system clipboard**, whatever
  register is selected. `space y`, `space Y`, `space p`, `space P` and `space R`
  are the cross-application path and are not redirected by `"`.
- **The blackhole register writes nothing and reads nothing.** Pasting from it is
  a stated refusal, not an empty paste.
- **No second text buffer.** A register holds a bounded fragment the user
  deliberately yanked — not a document copy. The text-authority guard's rules are
  the test of that, not a reviewer's judgement.
- **No dead chord.** `Ctrl-r` stays resync unless D2 resolves otherwise.
- **One owner per chord**, no shadowed arm.

## 5. Scope

### In scope

- A bounded register store with the unnamed register, `a`–`z`, `_` (blackhole)
  and `/` (search).
- `"` armed as a prefix whose captured name selects the register for the next
  verb.
- `y`, `p`, `P`, `R`, `d` and `c` honour the selected register.
- `d` and `c` yank to the unnamed register — M002's D1 discharged.
- `docs/tui.md` and the keymap box updated; the predecessor's "no register"
  statements corrected.

### Explicitly out of scope

- **`Ctrl-r` insert-register in Insert mode** unless D2 resolves otherwise. See
  §9.
- The `%` (file name), `#` (selections) and `:`/`.` (command) special registers.
- Durable registers. They do not survive a close or a reconnect.
- Registers shared between documents. One document is attached at a time
  (roadmap §3); the store is per-document presentation state.
- A register **inspector** UI. `"` plus a name is the whole surface.
- Changing what `space y`/`space p` do.

## 6. Required production changes

### Core/domain

None.

### Storage and migrations

None. Registers are presentation state.

### Protocol and DTOs

None.

### Runtime and concurrency

None. Register writes are synchronous over the snapshot the yank already holds.

### Frontend or operator surface

- `src/tui/document_session.rs`: the register store, its byte cap, its selected
  register, and its clearing.
- `src/tui/editor.rs`: `EditorPrefix` gains the register arm with its captured
  name; the yank/paste verbs resolve against the selected register; the
  blackhole and search registers' rules.
- `src/tui/commands/editor.rs`: the `"` arm and its name capture; the yank and
  paste arms route through the store; the App-layer seed call (D1).
- `src/tui/editor_effect.rs`: the clipboard verbs stay as they are; a register
  paste must not become a fourth `ClipboardVerb`.

### Security and authorization

No change. A register write reads bytes the daemon already authorized through
`try_snapshot`, and a register paste goes through `apply_local` like every other
edit.

### Documentation and static guards

- `docs/tui.md`: the `"` prefix, the register set, the bound, the clearing rule,
  the blackhole refusal, and which verbs honour a register.
- `scripts/check_tui_editor_text_authority.py` must be **re-run and must pass**.
  If it trips, the store's **shape** changes; the guard's rules are not widened.

## 7. Ordered work packages

### Work package A — The store, shaped to satisfy the text-authority guard

Intent: a bounded register store that is not a second buffer.

Required changes: the store and a named byte cap. Two shapes were considered.
*(i)* `BTreeMap<char, String>` — one entry per named register. *(ii)* A fixed
array indexed by the register's letter plus named fields for the special
registers.
**Recommendation: (i).** It is sparse (a user who yanks to `a` and `q` does not
allocate 26 slots), it iterates in a defined order if a future inspector needs it,
and it does not trip the guard's `Vec<String>` rule.
**This is a hard constraint, not a preference:** the guard treats a field holding
`Vec<String>` as a second text buffer, so a `Vec<String>` register store would
fail the build's guard step. If the implementation trips the guard, change the
shape — do not add an allowlist entry, which would weaken the rule for every
future field.

Acceptance evidence: the guard passes with no allowlist edit; a store past its
byte cap discloses the truncation; a lifecycle transition clears it.

### Work package B — The `"` prefix

Intent: select a register one level deep, in the same shape as `m`.

Required changes: `EditorPrefix` gains a register arm; `"` arms it; the next key
is captured as a register name and must be a valid one (`a`–`z`, `"`, `_`, `/`);
an invalid name disarms and reports which names exist. A **sticky field** was
considered and rejected: a selected register that survives an unrelated key is a
second grammar, and the prefix machine already exists to avoid exactly that.

Acceptance evidence: `"` then `a` then `y` yanks to `a`; `"` then an invalid
character disarms and reports the register set; `"` then `Esc` leaves nothing
selected.

### Work package C — Yank, delete and change route through the register

Intent: M002's D1 discharged.

Required changes: `y` writes the selected register (default unnamed). `d` and `c`
write the **unnamed** register regardless of the selected register — Helix's
delete is not register-targeted — and `Alt-d`/`Alt-c` write nothing. `R` replaces
from the selected register.

Acceptance evidence: `"ay` then `"ap` round-trips; `y` then `p` round-trips
through the unnamed register; `d` then `p` pastes what was deleted; `Alt-d` then
`p` pastes whatever the unnamed register held before; `"ap` after `"ay` in a
different document is refused because the store is per-document.

### Work package D — Paste semantics, and the D1 decision

Intent: decide what bare `p` reads, because today it reads the OS clipboard.

**D1 — bare `p`, the unnamed register, and the system clipboard.** Three
candidates.

*(a)* `p` reads the unnamed register; the unnamed register is **seeded from the
system clipboard when a document is opened**, and every `y` writes both the
unnamed register and the clipboard; `d`/`c` write the unnamed register only; the
`space` family always means the clipboard.
*(b)* `p` reads the unnamed register only; no seeding.
*(c)* `p` keeps reading the clipboard; registers are reachable only through
`"<reg>`.

**Recommendation: (a).** (b) silently breaks the one gesture users rely on for
cross-application paste — copy in a browser, press `p`, get the wrong text with
no explanation. (c) leaves the model half-applied: `"ap` would work while bare
`p` and `space p` remained identical, which teaches a distinction that does not
exist. (a) costs one App-layer seed call at document open and one sentence of
documentation, and it preserves today's `y` → paste-elsewhere behaviour exactly
while adding registers.

The seeding is an **interface point with the App layer**, which owns the
clipboard: the buffer cannot read it. The plan requires the seed to be a single
call at open, not a read per paste, so the paste path stays synchronous and the
buffer stays clipboard-free.

Acceptance evidence: opening a document then pressing `p` pastes the clipboard;
`y` then `p` round-trips; `d` then `p` pastes the deletion; `space p` pastes the
clipboard even after a `d`; the buffer never calls the clipboard itself.

### Work package E — Special registers

Intent: the two that are cheap and the one that is a refusal.

Required changes: `_` writes nothing and refuses a read with a stated message;
`/` reads the last accepted search pattern from the search line's own state, so it
must be a read-only projection rather than a copy.

Acceptance evidence: `"_d` deletes and leaves every register untouched; `"_p`
refuses with a message; `"/p` pastes the last accepted search pattern; with no
search yet, `"/p` refuses with a message.

### Work package F — D2, guards and documentation

**D2 — `Ctrl-r` insert-register against the guard block's resync.** `Ctrl-r` is
the mode-independent resync chord and the key authority guard asserts it **by
name**, so it cannot be silently shadowed. Two candidates.
*(a)* Keep `Ctrl-r` as resync and leave insert-register unbound, recording it as
the one Helix register gesture this TUI does not reach.
*(b)* Move resync to another chord and give `Ctrl-r` to insert-register.
**Recommendation: (a).** Resync is a lifecycle round trip that the guard block's
own documentation requires to be mode-independent — "save and resync are
mode-independent and always available" — and moving it is a rebinding decision
about a recovery affordance, not a milestone of a keybinding roadmap. The cost is
one insert-mode convenience; the register capability itself is complete without
it, because `"<reg>p` reaches every register from Normal mode.

Required changes: update the leaf guard's expectations for the new `"` arm; the
key authority guard's Normal-mode figure moves; document the register set, D1 and
D2.

Acceptance evidence: the key authority guard passes with the new count and no
shadowed arm; the doc-drift guard passes in both directions; `docs/tui.md` states
D2 and why.

## 8. Failure, cancellation, restart, and contention semantics

- **Partial failure.** A yank that cannot read one member skips it, as
  `yank_editor_ranges` already does. A paste whose transaction the controller
  rejects is reported and the document is unchanged.
- **Duplicate delivery.** None; synchronous.
- **Restart.** Registers are empty after a restart, by construction.
- **Cancellation races.** None; no async work added.
- **Stale generations.** Cleared by the same transitions that clear the undo
  history.
- **Bound reached.** A register write past the cap discloses; a paste from a
  truncated register is still a paste, and the truncation was already disclosed
  at write time.
- **Register selected but the store cleared.** The selection is cleared with the
  store, so a paste cannot silently fall back to the unnamed register.

## 9. Compatibility and migration

No storage, protocol, or configuration migration.

**Two behaviour changes, both decided above rather than discovered.**

1. **`d` and `c` now yank** into the unnamed register. They still do **not** touch
   the system clipboard, so a user's clipboard is never clobbered by a delete.
   This is M002's D1 discharged, not a new decision.
2. **`p` reads the unnamed register**, which is seeded from the clipboard at open
   and kept in sync by `y`. Under D1(a) this is indistinguishable from today for a
   user who only copies and pastes; it differs only after a `d`, where it now
   pastes the deletion — which is the behaviour Helix users expect and the reason
   registers exist.

**D2's compatibility surface:** none under (a), because nothing is rebound.
Under (b), `Ctrl-r` stops being resync on the editor route, which is a recovery
affordance moving and must be documented where the global bindings are.

Removal criteria for a transitional state: there is none; both changes are the
intended end state.

## 10. Required tests

### Focused unit tests

- The store: write, read, blackhole write and read, the byte cap, clearing.
- Register-name validation: every valid name, and an invalid character.
- The search register's projection from the search line's state.

### Integration tests

- `"ay` then `"ap` through the real dispatch path; the same for `b`.
- `d` then `p`; `Alt-d` then `p`; `space p` after a `d`.
- `"_d` leaves every register untouched; `"_p` refuses.
- The `"` prefix disarming on an invalid name.
- The seed: open a document with a known clipboard value, press `p`, observe it.

### Restart and recovery tests

None beyond the clearing assertion; the closure record must say the store is
in-memory by design.

### Contention and cancellation tests

- A multi-member yank writes one register entry, not one per member.

### Security and negative tests

- `check_tui_editor_text_authority.py` passes with **no allowlist edit**. If it
  trips, the store's shape changes.
- The buffer performs no clipboard call — the seed and the paste both go through
  the App layer, and the guard's filesystem/authority rules are the structural
  check.

### Migration and compatibility tests

- The `y` → paste-elsewhere path: `y` still writes the system clipboard, asserted
  directly.
- The `d` → clipboard-unchanged path: `d` does not write the clipboard, asserted
  directly. This is the test M002 wrote; M007 must keep it true for `d`'s new
  register write.

## 11. Required verification commands

```bash
cargo fmt --check
cargo test -p codegg --lib tui::editor
cargo test --test tui_render
python3 scripts/check_tui_editor_text_authority.py
python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_tui_editor_leaf_authority.py
python3 scripts/check_tui_editor_doc_drift.py
python3 scripts/check_comment_budget.py
bash scripts/verify.sh quick
```

## 12. Documentation updates

- `docs/tui.md`: the `"` prefix; the register set with `_` and `/` named; the byte
  cap; the clearing rule; which verbs honour a register and that the `space`
  family does not; D1's seeding rule; D2's outcome.
- The `space ?` keymap box follows.
- The predecessor's statements that the editor has "no register" must be
  corrected where they appear, because they are now false.

## 13. Acceptance criteria

- `"ay` yanks to register `a` and `"ap` pastes it.
- `y` then `p` round-trips through the unnamed register.
- `d` then `p` pastes the deleted text; `Alt-d` then `p` does not.
- `d` and `c` do not change the system clipboard; `y` still does.
- `space p` pastes the system clipboard regardless of the selected register.
- `"_d` leaves every register unchanged; `"_p` refuses with a message.
- Opening a document then pressing `p` pastes the clipboard.
- The text-authority guard passes with no allowlist edit; the key authority guard
  reports no shadowed arm; the doc-drift guard passes in both directions.
- D1 and D2 are resolved and recorded.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- The text-authority guard trips on the store's shape and the fix would be an
  allowlist entry. Change the shape or report; do not weaken the guard.
- D1's seeding cannot be done without the buffer reading the clipboard itself.
  That is an authority violation; report the interface instead.
- D2 has not been resolved by the owner. Do not move resync on a guess.
- A register paste appears to need a fourth `ClipboardVerb`. Report the shape; the
  verb enum describes *where the text goes*, and a register is not a place.
- The work expands into durable registers, the `%`/`#` registers, a register
  inspector, or cross-document registers.
- M002 is not yet landed. The D1 obligation this milestone discharges is M002's,
  and without it `c`/`d` do not exist to route.

## 15. Closure evidence required

- The text-authority guard's pass output with no allowlist edit, and the store's
  shape.
- The register round-trip evidence for a named register and the unnamed one.
- The clipboard-preservation evidence for `d`/`c` and the clipboard-write evidence
  for `y`, both asserted directly.
- The `space` family's independence from the selected register.
- The blackhole refusal and the search-register projection.
- D1's and D2's resolutions with their consequences.
- The key authority guard's summary with the new count and no shadowed arm.
- A statement that no migration, protocol variant, crate, or new authority was
  added.
- The commands actually run, labelled local or CI, with unrun items named.

## 16. Handoff notes

- **The text-authority guard's `Vec<String>` rule is the trap.** A register store
  written as a vector of strings fails the build's guard step. Use a map or a
  fixed array, and never widen the guard.
- **`Ctrl-r` is resync.** The guard block asserts it by name and it is
  mode-independent by design. Do not shadow it; D2 says so.
- **The `space` family is not register-aware.** That separation is what keeps
  cross-application paste predictable; do not make `space p` honour `"`.
- **`Alt-c`/`Alt-d` write nothing at all.** M002's promise, and the clipboard test
  it wrote is the assertion.
- **The buffer does not read the clipboard.** D1's seed is an App-layer call at
  open; a per-paste read would put clipboard authority in the buffer.
- **`"` is currently unbound and falls through to the composer.** Binding it is a
  small correctness win on its own.
- **Serial test constraint.** No environment mutation is introduced; nextest
  profile `ci` applies.
- **Preserve the user's working tree.** No generated asset is touched.
