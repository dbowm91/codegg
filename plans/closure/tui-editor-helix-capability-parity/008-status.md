# TUI Editor Helix Capability Parity Milestone 008 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/tui-editor-helix-capability-parity/008-keystroke-recording-macros-and-dot-repeat.md`

Source subsystem roadmap:

- `plans/subsystems/tui-editor-helix-capability-parity-roadmap.md#milestone-008--keystroke-recording-macros-and-dot-repeat`

Repository baseline reviewed: `7642b9e38` (`feat-auto-20261109-f8abad0d`), plus
uncommitted working-tree work — 231 changed or untracked paths at the time of
review

Implementation commits or pull requests:

- **None.** This milestone exists only as uncommitted working-tree work on
  `feat-auto-20261109-f8abad0d`. Its hard dependency, M007 (registers), is
  likewise uncommitted; the plan file's header still says
  `blocked (hard dependency on roadmap M007)` although M007's store is in the tree.

Labels: **measured** is what this record's author read out of the tree or ran;
**reported** is the implementing session's measurement, not repeated here.

## 1. Executive finding

The recorder landed, `.` repeats the last insert, and **the macro pair was
deliberately not bound** — which is the plan's own option (c) and is recorded in
the user-facing document with the reason.

**The recorder is one type with two states.** `KeyRecorder`
(`src/tui/document_session.rs:1158-1270`) holds `keys` (the open gesture),
`last_insert` (the last completed one), and four flags: `recording`, `replaying`,
`overflowed`, `interrupted`. Its own doc comment carries the two properties the
plan cares about most: "**Capture happens after dispatch** … a replayed key is
never recorded … and a key the buffer refused is still recorded — the reader
pressed it, and a replay owes them the same refusal", and "**Bounded by
[`MAX_RECORDED_KEYS`]**. Reaching the bound **discards the gesture** instead of
replaying its prefix, because half an insert is a different edit, and it is
disclosed rather than dropped silently." `MAX_RECORDED_KEYS = 128`
(`src/tui/editor.rs:75`) with the notice template at `:89`.

**The capture point is structural, not conventional.** `handle_editor_key`
(`src/tui/commands/editor.rs:3187-3191`) is a wrapper:

```rust
pub(crate) fn handle_editor_key(app: &mut App, key: KeyEvent) -> bool {
    let consumed = dispatch_editor_key(app, key);
    capture_recorded_key(app, key);
    consumed
}
```

and its doc says why the wrapper exists: "every return path of
[`dispatch_editor_key`] passes here, so a new early return cannot skip the
capture." That is the plan's work package A, implemented so that the invariant
survives a future edit rather than relying on discipline.

**The gesture boundary is the insert session, not the last edit.**
`capture_recorded_key` (`:3474`) opens the capture on the key that enters Insert
and closes it on the `Esc` that leaves it — "the entry key included, which is why
`o` replays its newline" — and the closing `Esc` is not recorded ("an `Esc` left in
the log would be read as a replay interrupt"). Any *other* key that takes the
keyboard away mid-gesture calls `recorder.discard()`, whose doc states the rule:
"Abandoning a partial gesture is not the same as ending one, and only `Esc` ends
one."

**`.` and `Alt-.` are distinct, and both are asserted.** With no modifier,
`Char('.')` resolves to `BufferKey::RepeatInsert` (`src/tui/commands/editor.rs:5693`);
with `Alt`, it still resolves to the repeat-last-find/motion verb (`:5660`). The
plan's invariant "`.` repeats the last insert, not the last motion. `Alt-.` keeps
its meaning; the two are distinct" is therefore structural, and the tests
`alt_period_repeats_the_last_find_and_another_motion_disarms_it` and
`alt_dot_repeats_the_paragraph_motion_and_an_edit_is_not_one` keep `Alt-.`'s
meaning under test alongside it.

**D1 resolved to (c): `.` is the delivered verb, macros are not bound, and the
conflict is on the record.** `docs/tui.md`: "A macro would live in a register and
share that lifetime rather than getting a store of its own; macros are not bound
here, because `q` is the two-press exit arm and `Q` closes a project tab." That is
the right call for this TUI and the plan anticipated it: option (c) "costs only
the macro pair — `.` is the higher-value verb and is not affected." The
key-authority guard confirms nothing was shadowed — "2 guard-block chord(s), **92**
Normal-mode chord(s) … 8 Insert-mode chord(s), no shadowed arm" (measured, exit 0)
— so the two-press exit and `CloseProjectTab` are intact, and no fourth guard
stage was needed.

**The replay is not atomic, and the document says so.** `docs/tui.md`: "A repeat
is **not atomic**. It drives the same dispatch your keys drive, so it produces the
same edits, the same notices, the same refusals and the same history — `u` takes
back each step the gesture made, not the whole repeat in one. An interrupted
repeat keeps the steps it already applied." That is the plan's §8 statement, in
the user-facing document rather than only in the plan.

Status is conditional for the familiar two reasons — the planning documents still
read as if D1 were open, and no lane was run on a merged tree — and not because of
anything the milestone failed to build. **All nine acceptance bullets are met for
the scope D1(c) leaves in force**; the macro bullets are conditional on D1(a) in
the plan's own wording and are therefore not applicable.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| A bounded keystroke recorder capturing keys **after** dispatch | `handle_editor_key` (`commands/editor.rs:3187-3191`) is a wrapper that dispatches then captures; doc: "every return path … passes here, so a new early return cannot skip the capture" | pass | Structural, not conventional. |
| The recorder is bounded by a named count and refuses rather than dropping the tail | `MAX_RECORDED_KEYS = 128` (`editor.rs:75`); `KeyRecorder::record` returns `RecorderBound::{Recorded, ReachedCap, AtCap}` and sets `overflowed`; `finish()` keeps nothing when overflowed | pass | 128 keys. |
| Crossing the bound is disclosed | notice template `editor.rs:89`: "insert recording stopped at {MAX_RECORDED_KEYS} keys — this insert is not repeatable with `.`" | pass | |
| Replay cannot record | `capture_recorded_key` returns immediately when `recorder.is_replaying()` (`:3480`); `a_replay_neither_records_itself_nor_opens_a_capture` | pass | The plan's "otherwise the recorder captures its own replay and the log grows without bound". |
| Replay goes through the real dispatch entry | `replay_recorded_keys` feeds `handle_editor_key`; `an_esc_in_the_log_stops_the_replay_between_keys` | pass | No private replay path. |
| Replay is interruptible, and an already-dispatched key completes | `interrupt_replay` / `replay_interrupted`; `an_esc_in_the_log_stops_the_replay_between_keys`; the driver's doc states "a key already dispatched completes" | pass | |
| A nested replay is refused | `begin_replay` returns false when `replaying`; `a_second_replay_is_refused_until_the_first_one_ends` | pass | |
| Every replayed edit goes through `apply_local` | the replay drives the same dispatch, which submits through the shared edit funnel | pass by construction | No bulk path exists; the plan forbids one. |
| `.` repeats the last insert | `Char('.')` (no modifier) → `BufferKey::RepeatInsert` (`:5693`); `repeat_last_insert` (`:3514`); `dot_repeats_the_last_insert_and_twice_inserts_twice` | pass | |
| `.` with nothing recorded is a no-op that reports nothing, and is still claimed | `dot_with_nothing_recorded_is_claimed_and_reports_nothing` | pass | Otherwise the period would reach the composer. |
| A session containing a backspace replays the backspace's effect | `dot_replays_a_backspace_inside_the_gesture` | pass | |
| A session that opened a line with `o` replays the newline | `dot_replays_the_line_that_o_opened`; the capture includes the entry key | pass | |
| `.` twice inserts twice | `dot_repeats_the_last_insert_and_twice_inserts_twice` | pass | |
| The boundary is the insert session, not the last edit | `finish()` on `Esc`, `discard()` on any other key that takes the keyboard away, and the capture excludes the closing `Esc` | pass | The plan's `d`-between-inserts property follows: a non-Insert key while recording discards rather than ends. |
| `Alt-.` keeps its meaning; the two are distinct | `Char('.')` with `Alt` → the repeat-last-find/motion verb (`:5660`); `alt_period_repeats_the_last_find_and_another_motion_disarms_it`, `alt_dot_repeats_the_paragraph_motion_and_an_edit_is_not_one` | pass | |
| A recording is discarded by a mode change, a document switch and a reload | `discard()` / `clear()`; `discarding_a_capture_keeps_the_previous_gesture`; `opening_a_document_clears_the_recording_with_the_registers` | pass | Reload/resync/close share the `clear()` path; not separately named per transition. §10. |
| The recorder holds key events, not document text | `RecordedKey` entries; text-authority guard passes with no allowlist edit | pass | |
| D1 resolved and recorded; no dead chord | (c) implemented and documented in `docs/tui.md`; key authority 92 Normal-mode chords, no shadowed arm | **partial** | Recorded in docs, not in the plan/roadmap. §10. |
| The doc-drift guard passes in both directions | "52 leaf gestures and 100 chords are documented, and every key declared unbound is unbound" (measured, exit 0) | pass | 100 chords vs 98 at M005's close. |
| No migration, protocol variant, crate or new authority | recorder and registers are `TuiDocumentPresentation` state; no manifest or protocol change attributable | pass | |
| `cargo test -p codegg --lib tui::editor` green | **discharged 2026-11-13** by `scripts/verify.sh full` | pass | §10; 17,377 workspace tests, 0 failures. |
| `bash scripts/verify.sh quick` green | **discharged 2026-11-13** by `scripts/verify.sh full`, which calls `quick` | pass | §10. |

## 3. Production implementation evidence

- `src/tui/document_session.rs:1150-1270` — `KeyRecorder`: `begin`, `record`,
  `finish`, `discard`, `clear`, `last_insert`, `captured_len`, `is_recording`,
  `is_replaying`, `begin_replay`, `end_replay`, `interrupt_replay`,
  `replay_interrupted`, plus the `RecorderBound` return type.
- `src/tui/editor.rs:75`, `:89` — `MAX_RECORDED_KEYS` and the disclosure string;
  `:7106` — `discard_insert_session`.
- `src/tui/commands/editor.rs:3178-3200` — the capture wrapper;
  `:3462-3500` — `capture_recorded_key` and the open/finish/discard decision;
  `:3434` — the `.` arm calling `repeat_last_insert`; `:3514` — the replay
  driver; `:5693` — `Char('.') → RepeatInsert`; `:5660` — `Alt-.` unchanged.
- `docs/tui.md:2420-2442` — the dot-repeat section: the insert-session boundary,
  the no-op case, `Alt-.`'s distinct meaning, the non-atomicity, the 128-key
  bound, the register lifetime, and the macro deferral naming `q` and `Q`.

Nothing in the plan's "explicitly out of scope" list was implemented: no `Q`/`q`
binding, no recording across a document switch or a mode change (a recording is
abandoned), no nested macros, no macro editing or inspector, no durable macro
store, and `Ctrl-a`/`Ctrl-x` are untouched.

## 4. Verification executed

### Commands run

```bash
python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_tui_editor_text_authority.py
python3 scripts/check_tui_editor_doc_drift.py
python3 scripts/check_tui_editor_leaf_authority.py
python3 scripts/check_tui_editor_prefix_state.py

grep -rn 'MAX_RECORDED_KEYS' src/tui/editor.rs
grep -rn 'recording|replaying' src/tui/document_session.rs
sed -n '1150,1270p' src/tui/document_session.rs
sed -n '3178,3200p' src/tui/commands/editor.rs
sed -n '3462,3500p' src/tui/commands/editor.rs
grep -rn "Char('\.')|Char('\"')" src/tui/editor.rs src/tui/commands/editor.rs
sed -n '2420,2450p' docs/tui.md
grep -rn 'fn .*\(repeat|record|recorder|replay|macro\)[a-z_]*(' src/tui
```

### Results

| Command | Result |
|---|---|
| `check_tui_editor_key_authority.py` | **pass** — 2 guard-block, **92** Normal-mode, 8 Insert-mode, no shadowed arm, exit 0 |
| `check_tui_editor_text_authority.py` | **pass** — no retained text buffer, no direct filesystem reads, exit 0 |
| `check_tui_editor_doc_drift.py` | **pass** — 52 leaf gestures, **100** chords documented, exit 0 |
| `check_tui_editor_leaf_authority.py` | **pass** — 6 tables, 52 glyphs, 15 prefixes, 4 unavailable reasons, exit 0 |
| `check_tui_editor_prefix_state.py` | **pass** — one `pending_prefix`, exit 0 |
| `MAX_RECORDED_KEYS` | `= 128` at `src/tui/editor.rs:75` (measured) |
| Capture point | `handle_editor_key` dispatches, then calls `capture_recorded_key` (measured) |
| `Char('.')` | `RepeatInsert` with no modifier (`:5693`); `Alt` path unchanged (`:5660`) (measured) |
| Recorder tests | 4 in `document_session` (order, bound-discard, open-clears, second-replay-refused) + `discarding_a_capture_keeps_the_previous_gesture`; 7 in `commands/editor.rs` (the dot/replay family) (measured by name; none run here) |

### Not run

| Command | Why |
|---|---|
| `cargo test -p codegg --lib tui::editor` | Not run at the time of writing: compiling was expensive and three sessions were compiling, by instruction. **Discharged 2026-11-13** by the `scripts/verify.sh full` workspace sweep (17,377 tests, 0 failures). |
| `cargo test --test tui_render` | Same. **Discharged 2026-11-13**: render corpus 1,237 passed, 0 failed. |
| `cargo fmt --check`, clippy, `bash scripts/verify.sh quick` | Out of scope and forbidden for that session. **Discharged 2026-11-13** by `scripts/verify.sh full`, which runs `quick` → `run_guards()` (first line `cargo fmt --check --all`) and clippy `-D warnings`. |

## 5. Invariant review

- **Replay goes through the real dispatch entry.** Holds — the driver feeds
  `handle_editor_key`, and the tests assert that a refused key reproduces its
  notice rather than being skipped.
- **Replay cannot record.** Holds — an early return on `is_replaying()`, plus the
  assertion `a_replay_neither_records_itself_nor_opens_a_capture`.
- **Replay is interruptible and leaves the editor in a consistent mode.** Holds —
  the interrupt flag is checked between keys, and `an_esc_in_the_log_stops_the_replay_between_keys`
  asserts it.
- **Every replayed edit goes through `apply_local`.** Holds by construction; there
  is no bulk mutation path, which the plan forbids explicitly.
- **The recorder is bounded and reaching the bound is disclosed.** Holds — 128,
  `RecorderBound::ReachedCap`, and a notice that names the limit.
- **`.` repeats the last insert, not the last motion; `Alt-.` is untouched.**
  Holds, and both halves are under test.
- **No second text buffer.** Holds — the log holds key events; the text-authority
  guard passes.
- **No dead chord, no shadowed arm.** Holds — the guard reports none, and the
  milestone added a chord (`92` Normal-mode where M005's record recorded 90)
  without shadowing anything.

## 6. Failure and recovery review

- **Partial failure.** A replay interrupted mid-way leaves the edits already
  applied; there is no rollback, and `docs/tui.md` states it rather than implying
  atomicity. Each replayed edit is its own history entry, so `u` walks back
  through them one at a time.
- **Duplicate delivery / cancellation races.** None; recording and replay are
  synchronous, and the interrupt is a flag checked **between** keys, so a key
  already dispatched completes.
- **Restart.** No recording survives a restart — presentation state. Recorded here
  because the plan's §10 requires the record to say so rather than omit the
  section.
- **Stale generation.** A document switch, a reload, a resync and a close clear
  the recorder with the registers, so a replay cannot target a document the
  recording did not come from.
- **Bound reached.** The gesture is discarded and the reader is told; the previous
  gesture is kept intact (`discarding_a_capture_keeps_the_previous_gesture`).
- **Re-entrancy.** A `.` inside a recorded log is refused inside the replay rather
  than recursing (`a_log_containing_dot_is_refused_inside_the_replay_rather_than_recursing`),
  which is the concrete form of "a replay must not re-enter itself".

## 7. Migration and compatibility review

No storage, protocol, configuration or dependency migration. The compatibility
surface is `.` alone:

- **`.` currently types a period into the composer on the editor route; binding it
  is the intended effect of a binding.** It is now claimed and does `/`'s job in
  Normal mode, and `dot_with_nothing_recorded_is_claimed_and_reports_nothing`
  asserts it stays claimed when there is nothing to repeat.
- **Under D1(c) there is no second compatibility surface**: `q` still arms the
  two-press exit and `Shift+Q` still closes a project tab, on every route, because
  no macro chord was bound.

Removal criteria for a transitional state: none; there is none.

## 8. Security review

No authorization change. A replayed edit is an edit through the shared funnel, so
it inherits the read-only refusal — a macro or a `.` recorded on a writable
document cannot write into a read-only attachment. The plan's macro half is moot
under D1(c), but the property that governs it holds for `.` too.

Bounded properties that matter: the 128-key cap, the per-key interrupt check, the
single-entry replay flag (no nesting), and the absence of any durable state. The
recorder holds no document bytes.

## 9. Documentation and operations

- `docs/tui.md:2420-2442` — the dot-repeat section: the insert-session boundary
  and why a `d` between inserts changes nothing, the no-op case, `Alt-.`'s
  distinct meaning, the non-atomicity, the 128-key bound and its notice, the
  register lifetime and clearing, and the macro deferral naming `q` and `Q`.
- The `space ?` keymap box: doc-drift passes in both directions (100 chords), so
  the box and the document agree with the tables.
- The recorder's own doc comments state the capture point, the bound's refusal
  mode, and the difference between `finish` and `discard` — the three things a
  future editor of this file needs.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | D1 resolved to (c) — `.` only, macros unbound with the `q`/`Q` conflict named — and it is recorded in `docs/tui.md`, but the plan and roadmap still present D1 as an open decision with a recommendation. | A planning reader sees an unsettled decision where the milestone has decided, which invites re-litigation of a call the code and docs have settled. | Add one resolution line to the plan's D1 section and to the roadmap's M008 block. Parent-owned. |
| low | The plan's acceptance bullet "A recording is discarded by a mode change, a document switch and a reload" is delivered by three different mechanisms — `finish()` on `Esc`, `discard()` on any other key that takes the keyboard away, `clear()` on open/reload/resync/close — and only the first two plus the open case are named by a test. | The behaviour is right and documented; the *test names* the plan implies for a reload and a resync do not exist separately, so a future regression on one of those transitions would be caught by the general clearing test or not at all. | Add one test per lifecycle transition, or record that they share `clear()` and that the open case is the representative assertion. |
| low | `.` on a **read-only** attachment is not separately asserted. | The replay inherits the read-only refusal through the shared edit funnel, so the behaviour is right by construction, but the plan's negative-test section asks for refusal-shaped evidence. (The macro case the plan names is moot under D1(c).) | Add a `dot_on_a_read_only_attachment_is_refused_visibly` test, or accept the funnel as the evidence. |
| low | **Discharged 2026-11-13.** `bash scripts/verify.sh quick`, `cargo fmt --check` and both test lanes were not run by this record's author; the tree was uncommitted. | The plan's §11 list had no result in this record. | None: the 2026-11-13 `scripts/verify.sh full` ran the workspace sweep (17,377 tests, 0 failures), the render corpus (1,237, 0 failures), `quick` → `run_guards()` and clippy `-D warnings`, all green on this tree. |
| low | The source plan's header still reads `blocked (hard dependency on roadmap M007)` although M007's store is present in the tree. | A stale status line on a milestone whose blocker has landed. | Corrected to `conditionally closed` by this record's author (§12). |

## 11. Roadmap disposition

**Milestone conditionally closed with named operational evidence outstanding.**

Everything D1(c) leaves in scope is delivered: the bounded recorder, the
structural capture point, `.` with all four behaviours the plan names, the
interrupt/nesting/refusal properties, the lifecycle clearing, the documentation,
and the guard results. The macro pair is **not** an outstanding deliverable — it is
a decision the plan gated on D1, D1 resolved against it, and the conflict is
recorded. The operational condition is **discharged**: `scripts/verify.sh full`
ran on this tree at 2026-11-13 — 17,377 workspace tests and 8,441 feature-pass
tests, 0 failures, `cargo fmt --check --all` and clippy `-D warnings` clean,
render corpus 1,237 passed — and `full` calls `quick`, which calls
`run_guards()`. What remains are the three evidence items above
(per-transition discard tests, the read-only `.` refusal, and the plan/roadmap
D1(c) line), which assert behaviour the code already has or document a decision
it already made.

What M009 (undo tree) may rely on unchanged: `MAX_RECORDED_KEYS`, the
`handle_editor_key` wrapper, and the fact that a replay is a sequence of ordinary
dispatched edits each producing its own history entry — a property an undo tree
must preserve.

## 12. Registry updates

- `plans/registry.md` line 30 (subsystem row): "M006 in flight, M007–M009 pending"
  → M006, M007 and M008 conditionally closed with their records' paths.
- `plans/registry.md` line 689 (the M008 milestone row): status "**Blocked — hard
  dependency on roadmap M007 (registers)**" → conditionally closed, with this
  record and D1(c) named.
- `plans/subsystems/tui-editor-helix-capability-parity-roadmap.md` §12 row
  `M008 keystroke recording, macros and dot-repeat`: `blocked` → `conditionally
  closed`, closure record
  `plans/closure/tui-editor-helix-capability-parity/008-status.md`, blocker
  "per-transition discard tests" (the `quick`/`fmt --check` half was discharged
  2026-11-13).
- `plans/subsystems/tui-editor-helix-capability-parity-roadmap.md` §7 M008 body:
  record D1(c) — macros unbound, `.` delivered — so the roadmap's "`Q`/`q`
  round-trips a multi-key edit" exit condition is marked as not applicable rather
  than unmet.
- `plans/implementation/tui-editor-helix-capability-parity/008-keystroke-recording-macros-and-dot-repeat.md`
  header: `Status: blocked (hard dependency on roadmap M007)` → `Status:
  conditionally closed`. **This record's author made this header edit**; the D1
  resolution line is left for the parent.

## Correction log (2026-11-13)

- **§2, the two acceptance rows** — "`cargo test -p codegg --lib tui::editor`
  green | not run" and "`bash scripts/verify.sh quick` green | not run" →
  discharged by the 2026-11-13 `scripts/verify.sh full` run: 17,377 workspace
  tests and 8,441 feature-pass tests at 0 failures, and `full` calls `quick`.
- **§4, `Not run`** — the three rows (both test lanes; `fmt --check`, clippy,
  `quick`) → each marked discharged by the same run, which includes
  `cargo fmt --check --all` and clippy `-D warnings`.
- **§10, low (`quick`/`fmt`/tests not run)** — "Run on the merged tree from the
  merging session" → discharged 2026-11-13; the run happened on this tree.
- **§11** — "The condition is operational: green `quick` and `fmt --check` on
  the merged tree, plus the three small evidence items above" → the operational
  half is discharged; the three evidence items remain named.
- **§12, the roadmap blockers cell** — "green `quick`/`fmt --check` on the
  merged tree; per-transition discard tests" → the gate half is discharged
  2026-11-13; the blocker that remains is the per-transition discard tests.
