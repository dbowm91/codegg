# TUI Editor Helix Capability Parity Milestone 008 — Keystroke Recording: Dot-Repeat and Macros

Status: conditionally closed
Repository baseline: `7642b9e381d9c91d650fa375be2a7aa0aae9bcd8`

Source roadmap:

- `plans/subsystems/tui-editor-helix-capability-parity-roadmap.md#milestone-008--keystroke-recording-macros-and-dot-repeat`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#29-system-invariants`

Applicable ADRs:

- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`

Primary class: capability

## 1. Objective

Add a bounded **keystroke recorder** and use it for the two verbs that need one:
`.` (repeat the last insert) and `Q`/`q` (record and replay a macro). Dot-repeat
is the unconditional deliverable; the macro pair is gated on D1 because both of
Helix's chords are already owned by global actions.

## 2. Why this milestone is blocked, and what unblocks it

**Hard dependency: roadmap M007 (registers).** A macro records to a register, so
the register store must exist before a macro can be stored anywhere. There is no
sensible interim: a macro store of its own would be a second bounded collection
with the same lifetime and the same clearing contract, which is a second answer
to a question M007 already answers.

Everything else is ready:

- The dispatch entry is a single function (`handle_editor_key`), so a replay can
  go through the same path the keyboard does. That is what keeps the key
  authority guard's model true instead of making replay a second, unguarded
  keymap.
- `InsertSession` already establishes that a typing burst is **one** undo step and
  that a run is a contiguous insertion, so the boundary `.` repeats is a concept
  the model already has.
- `MAX_EDITOR_PENDING_PREFIX`, the command line and search line bounds, and the
  undo byte cap are the established precedent for a bounded collection with a
  disclosed limit.

## 3. Current implementation evidence

At the baseline:

- `.` is **unbound** in Normal mode. Only `Alt-.` has an arm
  (`repeat_last_motion`), so bare `.` falls through and types a period into the
  composer.
- `InsertSession` records the **transactions** a typing burst produced, not the
  keystrokes. It is explicitly a contiguous run of pure insertions, and a
  deletion, replacement or off-run insertion ends the run. A transaction's edits
  are expressed in the coordinates of one base text, so replaying them at a
  different cursor position would apply them at the wrong offsets — which is why
  `.` cannot be built from the session's transactions and needs the keystrokes.
- **`q` is `InputAction::Quit`** and **`Shift+Q` is `InputAction::CloseProjectTab`**
  in the global default bindings. Both are live on the editor route today. AGENTS.md
  records the two-press `q` exit as a deliberate safety property, and the
  predecessor's own key authority guard documentation names the global map as a
  thing a later stage must not shadow.
- The key authority guard reports **2 / 74 / 8** with no shadowed arm, and its
  model is three ordered stages — the guard block, the Normal table, the Insert
  table. A chord claimed by an earlier stage makes a later arm dead.
- There is no recorder of any kind, and no bounded key-event collection anywhere
  in the editor path.

## 4. Invariants that must not regress

- **Replay goes through the real dispatch entry.** A replayed key produces the
  same effect as the same key pressed by the user, including its notices and its
  refusals. A private replay path would be a second keymap.
- **Replay cannot record.** A macro that starts a recording is refused; otherwise
  the recorder captures its own replay and the log grows without bound.
- **Replay is interruptible.** `Esc` stops a replay and leaves the editor in a
  consistent mode.
- **Every replayed edit goes through `apply_local`.** A replay is not a bulk
  document mutation and gets no shortcut.
- **The recorder is bounded** by a named count, and reaching it is disclosed —
  a macro that silently drops its tail is worse than one that refuses.
- **`.` repeats the last insert, not the last motion.** `Alt-.` keeps its
  meaning; the two are distinct and the plan asserts the distinction.
- **No second text buffer.** The recorder holds key events, not document text.
- **No dead chord, no shadowed arm.**

## 5. Scope

### In scope

- A bounded keystroke recorder capturing keys **after** dispatch.
- `.` repeating the last insert session.
- `docs/tui.md` and the keymap box updated.
- The macro pair **only if D1 resolves to bind it**.

### Explicitly out of scope

- **`Q`/`q` unless D1 resolves otherwise.** See §9.
- Recording across a document switch, a reload or a mode change. A recording is
  discarded rather than continued.
- Nested macros, macro editing, or a macro inspector.
- Replaying a macro from a *durable* store. Macros live in a register and share
  its lifetime.
- `Ctrl-a`/`Ctrl-x` increment/decrement, which share the numeric-grammar question
  but not this substrate (roadmap §3).
- Any change to `Alt-.`.

## 6. Required production changes

### Core/domain

None.

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

None. Recording and replay are synchronous. Replay drives the existing dispatch
synchronously, one key at a time, with an interrupt check between keys.

### Frontend or operator surface

- `src/tui/document_session.rs`: the recorder — a bounded key-event log with a
  `recording` flag and a `replaying` flag.
- `src/tui/commands/editor.rs`: capture after dispatch; the `.` arm; the replay
  driver; the interrupt check; the D1-gated `Q`/`q` arms if they are bound.
- `src/tui/editor.rs`: the recorder's cap constant and the replay's refusal
  messages.

### Security and authorization

No change. A replayed edit is an edit through `apply_local` and inherits
read-only refusal, so a macro recorded on a writable document is refused on a
read-only one rather than replaying into it.

### Documentation and static guards

- `docs/tui.md`: `.` and (if bound) `Q`/`q`; the bound; that replay is
  interruptible; that a macro lives in a register and shares its lifetime.
- If `Q`/`q` are bound, the key authority guard's model gains the stage that makes
  an editor-route override explicit rather than accidental, with a `--self-test`.

## 7. Ordered work packages

### Work package A — The recorder

Intent: a bounded log of key events with two flags and a stated capture point.

Required changes: the log, its cap, `recording`, `replaying`. Capture happens
**after** dispatch, so a replayed key is not recorded and a key the buffer
refused is still recorded (the user pressed it, and replay must reproduce the
refusal, not silently skip it).

Acceptance evidence: recording captures every Normal and Insert key in order;
replay does not capture; the cap refuses with a disclosure; a document switch, a
reload and a mode change each discard the recording.

### Work package B — `.` repeats the last insert

Intent: the unconditional deliverable.

Required changes: the recorder marks the span of the last Insert-mode session —
entering Insert begins a span, leaving it ends the span — and `.` replays that
span's keys at the current cursor. A `.` with no recorded insert is a no-op that
reports nothing.

Acceptance evidence: typing `abc` then `Esc` then `.` inserts `abc` at the new
cursor; a session containing a backspace replays the backspace's effect; a
session that opened a line with `o` replays the newline as well; `.` after only
motions is a no-op; `.` twice inserts twice.

**The boundary is the insert session, not the last edit.** A `d` between two
inserts does not change what `.` repeats, which is Helix's behaviour and the
reason the marker is a session span rather than "the last thing that happened".

### Work package C — The replay driver

Intent: one driver, shared by `.` and by macros.

Required changes: a driver that feeds recorded keys back through
`handle_editor_key`, checks the interrupt flag between keys, and stops on `Esc`.
It must not re-enter itself: a replay in progress refuses a nested replay with a
notice.

Acceptance evidence: a replay stops on `Esc` and leaves the editor in a
consistent mode; a replay of a key the buffer refuses produces the same notice a
user would see; a nested replay is refused; a replay of a macro that would exceed
the undo byte cap is bounded by the existing cap rather than growing it.

### Work package D — D1: the macro chords

Intent: decide the macro gesture, because both Helix chords are owned.

**D1 — `Q`/`q` against `CloseProjectTab` and `Quit`.** Three candidates.

*(a)* Bind `Q` (record) and `q` (replay) in Normal mode on the editor route,
accepting that `Quit` and `CloseProjectTab` stop working there. `Ctrl+c` still
arms the exit and `:q` still closes the document, so the safety property survives
by another spelling; `Shift+Q`'s panel exit is unaffected when the tree is
focused, because the tree's keys are answered first.
*(b)* Bind a different chord pair for macros and record the divergence.
*(c)* Do not bind macros; deliver `.` only and record the conflict.

**Recommendation: (a)**, with the guard change it requires. The editor route is a
distinct screen where `:q` is the documented close and `Ctrl+c` is the documented
exit, so both global actions have a second spelling there; and a private macro
chord would be a dialect a Helix user cannot guess. (c) is the fallback if the
owner will not accept the narrowing, and it costs only the macro pair — `.` is
the higher-value verb and is not affected.
If (a) is chosen, the plan requires: a fourth stage in the key authority guard's
model naming the editor-route overrides, a `--self-test` proving the guard rejects
an injected *unrecorded* override, and `docs/tui.md` stating what the editor route
takes from the global map.

Acceptance evidence: D1 is resolved and written into this plan and the roadmap;
whichever option is chosen, no dead chord exists and the guard's model matches the
dispatch chain.

### Work package E — Macros, if D1 resolves to (a)

Intent: record to a register and replay from it.

Required changes: `Q` starts a recording into the register selected by `"` (or
the unnamed register), `Q` again stops it, `q` replays it. A recording into the
blackhole register is refused with a reason.

Acceptance evidence: `"aQ` then a multi-key edit then `Q` then `"aq` reproduces
the edit; recording into `_` is refused; a recording interrupted by a document
switch is discarded with a notice; a macro that opens Insert replays through
Insert.

### Work package F — Guards and documentation

Required changes: update the key authority guard (and its model, if D1 is (a));
update the doc-drift figures; document `.` and the macro outcome.

Acceptance evidence: the key authority guard passes with no shadowed arm; the
doc-drift guard passes in both directions.

## 8. Failure, cancellation, restart, and contention semantics

- **Partial failure.** A replay that is interrupted mid-way leaves the document
  with the edits already applied; there is no rollback, and the plan states that
  rather than implying atomicity. Each replayed edit is its own history entry, so
  `u` walks back through them.
- **Duplicate delivery.** None; synchronous.
- **Restart.** No recording survives a restart.
- **Cancellation races.** `Esc` sets the interrupt flag between keys; a key
  already dispatched completes.
- **Stale generations.** A document switch discards the recording, so a replay
  cannot target a document the recording did not come from.
- **Bound reached.** The recorder refuses at its cap and discloses; it does not
  drop the tail.

## 9. Compatibility and migration

No storage, protocol, or configuration migration.

**The compatibility surface is `.` and, under D1(a), two global chords.**

- `.` currently types a period into the composer on the editor route. Binding it
  is the intended effect of a binding.
- Under D1(a), `q` stops arming the exit and `Shift+Q` stops closing a project tab
  **on the editor route only**. `Ctrl+c` remains the exit arm and `:q` remains the
  close, so the safety property AGENTS.md records is preserved by another
  spelling — but the narrowing must be documented where the global bindings are,
  not only in the editor's section.
- Under D1(c) there is no second compatibility surface.

Removal criteria for a transitional state: none; there is no transitional state.

## 10. Required tests

### Focused unit tests

- The recorder: capture order, cap refusal, discard on mode change, discard on
  document switch.
- The insert-span marker: begins on Insert entry, ends on Insert exit, is not
  moved by an intervening delete.

### Integration tests

- `.` through the real dispatch path, for a plain insertion, a backspace, an
  `o`-opened line, and a second `.`.
- The replay driver: interrupt on `Esc`, refusal of a nested replay, a refused key
  reproducing its notice.
- Under D1(a): `Q`/`q` round-trip, blackhole refusal, interrupted recording
  discard.

### Restart and recovery tests

None. No durable state; the closure record must say so rather than omit it.

### Contention and cancellation tests

- An interrupted replay leaves the edits already applied and `u` walks back
  through them one at a time.

### Security and negative tests

- `check_tui_editor_text_authority.py` passes with no allowlist edit.
- A macro recorded on a writable document is **refused** on a read-only one,
  rather than replaying into it.
- A replay cannot start a recording.

### Migration and compatibility tests

- `.` no longer reaching the composer, asserted.
- Under D1(a): the global actions' other spellings still work, and a guard
  `--self-test` rejects an injected unrecorded override.

## 11. Required verification commands

```bash
cargo fmt --check
cargo test -p codegg --lib tui::editor
cargo test --test tui_render
python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_tui_editor_text_authority.py
python3 scripts/check_tui_editor_doc_drift.py
python3 scripts/check_comment_budget.py
bash scripts/verify.sh quick
```

If D1 resolves to (a), also run
`python3 scripts/check_tui_editor_key_authority.py --self-test` and record its
output.

## 12. Documentation updates

- `docs/tui.md`: `.`; the insert-session boundary it repeats; that `Alt-.` is a
  different verb; that a replay is interruptible and not atomic; the macro outcome
  from D1, including what the editor route takes from the global map if (a).
- The `space ?` keymap box follows.
- The recorder's doc comment states that capture happens after dispatch, and why.

## 13. Acceptance criteria

- Typing `abc`, `Esc`, then `.` inserts `abc` at the new cursor.
- `.` after only motions is a no-op that reports nothing; `.` twice inserts twice.
- A replay stops on `Esc` and leaves the editor in a consistent mode.
- A nested replay is refused with a notice.
- The recorder refuses at its cap and discloses.
- A recording is discarded by a mode change, a document switch and a reload.
- A macro recorded on a writable document is refused on a read-only one.
- D1 is resolved and recorded; under (a), `"aQ` … `Q` … `"aq` reproduces the edit
  and the key authority guard's model matches the dispatch chain.
- The doc-drift guard passes in both directions.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- D1 has not been resolved by the owner. Do not bind `Q`/`q` on a guess; `.` is
  the safe deliverable and the plan says so.
- Roadmap M007 has not landed. The macro half has no store.
- A replayed key cannot go through `handle_editor_key` without re-entering the
  recorder. Report the shape; a private replay path is not acceptable.
- `.` appears to need the insert session's transactions rather than its keys.
  That cannot be correct across a cursor move; report the finding.
- The work expands into nested macros, durable macros, or a macro inspector.
- Someone proposes making replay atomic. That would require a bulk mutation path
  outside `apply_local`; report rather than adding one.

## 15. Closure evidence required

- D1's resolution and its recorded consequence.
- `.`'s behaviour for each of the four cases named in §10.
- The interrupt, nested-refusal and refused-key evidence.
- The recorder's cap and discard evidence.
- The read-only refusal for a replayed macro.
- If D1 is (a): the guard model change, its `--self-test` output, and the
  documentation of what the editor route takes from the global map.
- A statement that no migration, protocol variant, crate, or new authority was
  added.
- The commands actually run, labelled local or CI, with unrun items named.

## 16. Handoff notes

- **`.` is the deliverable; macros are the option.** They share the recorder but
  not their availability, and the plan is structured so `.` can land alone.
- **`q` is `Quit` and `Shift+Q` is `CloseProjectTab`.** Both are live on the
  editor route today. D1 is the decision and it is not the implementer's.
- **Replay is not atomic.** Each replayed edit is its own history entry. Do not
  claim otherwise in the docs, and do not build a bulk path to make it true.
- **Capture after dispatch.** Capturing before dispatch records keys the buffer
  refused as if they had worked, and replay would then reproduce a different
  history.
- **The insert-session boundary, not the last edit.** A `d` between two inserts
  does not change what `.` repeats.
- **Serial test constraint.** No environment mutation is introduced; nextest
  profile `ci` applies.
- **Preserve the user's working tree.** No generated asset is touched.
