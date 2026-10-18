# TUI Editor Helix Parity Milestone M003 — Character-finding motions

Status: closed — closure `plans/closure/tui-editor-helix-parity/003-status.md`

Outcome: `f`/`F`/`t`/`T` search the whole document rather than the cursor's
line, armed through M002's `EditorPrefix` so no new presentation field was
added for the target. `t`/`T` land **on** the target as §3/AC2 require, which at
this milestone leaves them cursor-equivalent to `f`/`F` — the real distinction is
a selection property and belongs with M007. `Alt`-`.` repeats the last find and
is cleared by any other motion. The bounded scan discloses `Absent` vs `Bounded`
in the status row. Two real defects were found and fixed: a per-byte scan that
could not read a byte inside a multi-byte character (79 s → 0.22 s once chunked),
and an existing M001 test that asserted `f`/`t`/`F`/`T` were unclaimed. Two
injection-harness false positives were also corrected — a non-zero exit from a
failed *build* was being scored as evidence that a test was non-vacuous.

Repository baseline: `719cfeb5d` (branch `feat/auto-20261017-80a41db7`)

Source roadmap:

- `plans/subsystems/tui-editor-helix-parity-roadmap.md#7-milestones` (M003)

Long-term requirements:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`

Applicable ADRs:

- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`

Primary class: capability

## 1. Objective

Implement Helix's character-finding motion family — `f`, `F`, `t`, `T`, plus
`Alt-.` to repeat the last one — as bounded, disclosure-carrying motions over
the editor replica.

## 2. Why this milestone is ready

- Hard dependency M002 closed: `f` and `Alt-.` both consume a following key and
  therefore register against the unified prefix machine.
- ADR-0011 accepted. These motions are pure reads of the existing replica plus a
  cursor move: no new authority, no protocol, no dependency.
- The bounded-scan and disclosure contract already exists in this module and is
  reused rather than invented.

This is the highest-value motion gap in the survey: CodeGG currently has **no**
way to jump to a character, so reaching a symbol on a nearby line costs O(n)
`j`/`k` presses.

## 3. Current implementation evidence

### No character motion exists

`EditorCommand` (`src/tui/editor.rs`) contains exactly these motions:
`MoveLeft`, `MoveRight`, `MoveUp`, `MoveDown`, `LineStart`, `LineEnd`,
`BufferStart`, `BufferEnd`, `WordForward`, `WordBackward`, `WordEnd`,
`HalfPageDown`, `HalfPageUp`, plus the insert-entry and delete commands.
There is no find-char motion, and no `f`/`t` key is claimed in
`handle_normal_key`.

Word motions exist (`word_forward`, `word_backward`, `word_end`) and are
character-based with no long-word variant — `W`/`B`/`E` belong to M005.

### The bounded-scan contract to inherit

The module already bounds document scans and **discloses** when it stopped
early. `MAX_EDITOR_SEARCH_BYTES` (2 MiB) bounds the `/` search, and
`SearchOutcome` carries `truncated` and `bounded` flags with the rationale
recorded in the source:

> Beyond this bound the search reports that it covered only the leading window
> rather than hanging on a huge file.

> A truncated match list that looks complete is the same defect as unvalidated
> LSP coverage.

A find-char motion that scans the whole replica on every keystroke would
reintroduce exactly the unbounded-render-path problem the predecessor roadmap's
viewport milestone had to defend against. The existing bound and disclosure
shape must be reused, not widened silently.

### How a motion becomes a cursor move

`EditorCommand` motions are resolved by `apply_motion` in `src/tui/editor.rs`
and moved into the selection model through
`HelixVerb::Moved(EditorCommand)`. `handle_normal_key` reaches that path via
`helix_motion`. Extending the motion enum is therefore the natural integration
point; the Helix verb wrapper needs no new variant.

`half_page_target` shows the existing precedent for a motion that resolves
against a bounded window rather than the whole document.

### Key-consumption subtlety

`f` takes the **next key as data**, not as a command. This is categorically
different from M002's prefix leaf resolution, where a non-matching second key
disarms and is then resolved normally. After `f`, the next key must be captured
as a target character and the prefix machine must not re-interpret it. `Alt-.`
composes with the same machine without arming a prefix.

### Helix semantics to match exactly

From <https://docs.helix-editor.com/keymap.html>:

- Unlike vi, `f`, `F`, `t`, and `T` are **not confined to the current line**.
- `t` (find till next char) and `T` (find till previous char) leave the cursor
  **on** the target character.
- `F` (find previous char) moves to the target character.
- `Alt-.` repeats the last motion, where "motion" means `f`, `t`, `m`, `[`, or
  `]` — of which only `f`/`t` exist today, so the repeatable set starts as
  `f` and `t`.

## 4. Invariants that must not regress

- Motions never mutate the replica. Find-char is a read plus a cursor move.
- Every resolved offset is clamped to the document and to a UTF-8 character
  boundary before it reaches the renderer or the cursor. Multi-byte targets must
  not slice mid-codepoint.
- Scans stay bounded by `MAX_EDITOR_SEARCH_BYTES` or an equivalent documented
  bound, and **disclose** when the scan stopped early. A motion that silently
  fails on a large file is a defect.
- `f`/`t` capture the following key as data; they must not swallow a following
  printable character intended for the composer in any path.
- `:` and `/` remain outranking all normal-mode keys, including a pending
  find-char target.
- Insert-mode printable characters are never consumed.
- No mutation outside `DocumentController::apply_local`; motions report no
  effect beyond a cursor move.
- `d` continues to claim its chord even with nothing selected.

## 5. Scope

### In scope

- `f` find next char, `F` find previous char, `t` find till next char, `T` find
  till previous char, all spanning line boundaries.
- A pending find-char target state composing correctly with M002's prefix
  machine.
- `Alt-.` repeating the last find motion.
- Clearing the last-motion record on any other motion.
- Bounded scan with `bounded`/no-match disclosure consistent with `SearchOutcome`.
- Visible pending-target feedback consistent with the M002 pending indicator.

### Explicitly out of scope

- Long-word `W`/`B`/`E`, smart-home `gs`, textual `gj`/`gk` — M005.
- Any change verb built on these motions — M004.
- Unimpaired `[d`/`]d` and the rest of the unimpaired layer — M006.
- Macros, registers, `.` repeat-**insert** (distinct from `Alt-.` repeat-motion).
- Any syntax-tree-dependent find.

## 6. Required production changes

### Core/domain

Extend the motion vocabulary in `src/tui/editor.rs` with the four find-char
motions, carrying the target character and direction. Resolve them through the
existing motion application path.

### Storage, protocol, runtime

None. No protocol change, no migration.

### Frontend surface

- Bind `f`, `F`, `t`, `T` in `handle_normal_key` (or its M002 successor), arming
  a pending find-char target rather than resolving immediately.
- Register `Alt-.` to repeat the last find motion.
- Compose with M002's prefix machine: arming a find target must disarm any
  pending prefix, and a pending target must consume exactly one key as data.
- Record the last motion, cleared by any other motion.

Design notes:

- The "not confined to the current line" rule is the behavior most likely to be
  implemented incorrectly by analogy with vi. It is stated in the Helix docs
  precisely because it differs, and cross-line cases must be covered by tests.
- `t`/`T` landing **on** the target rather than before it is the second easiest
  thing to get wrong. Both are explicitly specified above and must have
  dedicated tests.
- Use the existing bounded-scan helper and disclosure shape where it fits; if a
  dedicated bounded scan is needed, mirror `SearchOutcome`'s `truncated` and
  `bounded` semantics rather than inventing a quieter one.

### Security and authorization

None. These are reads of the already-authorized replica.

### Documentation and static guards

- Document the bounded-scan bound and its disclosure in the motion's own docs,
  matching the rationale style already used for `MAX_EDITOR_SEARCH_BYTES`.
- `docs/tui.md` "Editor language features" gains the four motions and
  `Alt-.`.

## 7. Ordered work packages

### Work package A — Bounded find-char scan

Intent: a reusable, bounded, disclosure-carrying primitive that finds a target
character relative to the cursor, forward or backward, across lines.

Required changes: a scan bounded like the existing search, returning either a
resolved offset or an explicit "not found within bound" outcome.

Acceptance evidence: unit tests for found-forward, found-backward, target at
buffer start/end, target absent, and a multi-byte target. A test that the scan
stops at the bound and reports it rather than hanging.

### Work package B — The four motions

Intent: `f`/`F`/`t`/`T` with Helix's exact landing semantics.

Required changes: extend `EditorCommand` and `apply_motion`.

Acceptance evidence: per-motion tests for found and not-found, including
cross-line cases in both directions, and the landing-position tests for `t`/`T`.

### Work package C — Key binding and target capture

Intent: arm and consume the pending target, composing with the prefix machine.

Required changes: bind the four keys; capture exactly one following key as the
target.

Acceptance evidence: `f` followed by a character finds that character; the
target state disarms after one key; a target capture never intercepts `:` or `/`
while those lines are open; a multi-byte target typed in one event is handled,
including a terminal encoding that reports a surrogate or a decomposed unit —
report what the encoding actually provides rather than assuming.

### Work package D — `Alt-.` repeat

Intent: repeat the last find motion, and only find motions.

Required changes: record the last find motion; bind `Alt-.`; clear the record on
any other motion.

Acceptance evidence: `f x` then `Alt-.` repeats; a intervening different motion
clears the record; with no recorded motion `Alt-.` is a safe no-op that does not
swallow a composer keystroke.

### Work package E — Disclosure and documentation

Intent: the bound and its disclosure are visible and documented.

Required changes: surface not-found-within-bound distinctly from not-found;
update `docs/tui.md` and `architecture/tui.md`.

Acceptance evidence: a test that a target absent from the scanned window is
distinguishable from one absent from the document, if the implementation
distinguishes them.

## 8. Failure, cancellation, restart, and contention semantics

- **Bound respected.** A find on a file larger than the bound must return
  promptly with a disclosed outcome. Test with an adversarial fixture.
- **No partial state.** If the target capture or the scan cannot complete, the
  pending state must disarm so the user is never stuck.
- **No async surface.** These motions are synchronous reads; restart,
  cancellation, and contention semantics are unchanged and are not applicable.
  State that explicitly in the closure.
- **Empty and tiny buffers** must not panic; offsets are clamped by existing
  helpers.

## 9. Compatibility and migration

No storage, protocol, or configuration migration.

**User-visible behavior change:** four previously unclaimed keys become
claimed. This narrows the set of Normal-mode keys that fall through to the
composer — `f`, `t`, `F`, `T`, and `Alt-.` will no longer reach the input box in
Normal mode. That is intended and is the reason `Alt-.` must remain a safe
no-op when there is no recorded motion. Record the narrowing in
`docs/tui.md`.

## 10. Required tests

### Focused unit tests

- Bounded scan: forward, backward, boundary, absent, multi-byte, bound-exceeded.
- Each of `f`/`F`/`t`/`T`: found, not-found, cross-line, and the correct landing
  position — especially `t`/`T` landing on the target.
- Target capture: exactly one key consumed; disarms on any non-character key.
- `Alt-.`: repeats; clears on another motion; safe no-op with empty record.

### Integration tests

- `cargo test --test tui_main_buffer` — editor route regression surface.

### Adversarial / performance tests

- A fixture larger than the scan bound completes promptly and reports the bound
  rather than hanging.

### Restart, contention, migration

Not applicable. State explicitly in the closure.

### Security and negative tests

- Find-char never mutates the replica.
- No resolved offset lands mid-codepoint, verified with a multi-byte fixture.

### Non-vacuity proof

The cross-line tests must be shown to fail against an implementation that
confines the search to the current line, and the `t`/`T` landing tests against
one that lands before the target. Otherwise a vi-shaped implementation would
pass a Helix-specified suite.

## 11. Required verification commands

```bash
# narrow first
cargo test -p codegg --lib tui::editor
cargo test -p codegg --lib tui::commands::editor
cargo test -p codegg --test tui_main_buffer

# terminal panel shares the dialect — confirm unaffected
cargo test -p codegg --test interactive_terminal_tui

# compile + guards
cargo ck
scripts/verify.sh quick

# formatting and linting
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

## 12. Documentation updates

- `docs/tui.md` "Editor language features": add the four motions and `Alt-.`,
  and note the composer fallthrough narrowing.
- `architecture/tui.md`: the motion vocabulary and the bounded-scan contract.
- `CHANGELOG.md`: new user-visible motions.

## 13. Acceptance criteria

1. `f`, `F`, `t`, `T` all work across line boundaries.
2. `t` and `T` land **on** the target character.
3. `Alt-.` repeats the last find motion and is disarmed by any other motion.
4. Every scan is bounded and discloses when the bound was reached.
5. No resolved offset is ever mid-codepoint.
6. The pending target consumes exactly one key and never intercepts `:`/`/` or
   Insert-mode input.
7. `Alt-.` with no recorded motion does not swallow a composer keystroke.
8. Terminal-panel behavior is unchanged.
9. `scripts/verify.sh quick`, `cargo fmt --check`, and clippy `-D warnings` pass.

## 14. Stop conditions

Stop and report rather than improvise when:

- Implementing cross-line search appears to require unbounded scanning. The bound
  is part of the contract; do not remove it to make a case pass.
- The pending target state cannot compose with M002's prefix machine without
  ambiguity — that indicates a design flaw in one of them.
- The change would require a new effect variant in `BufferKey`.
- A guard cannot be made non-vacuous.
- Repository evidence contradicts the bounded-scan disclosure precedent already
  established in this module.

## 15. Closure evidence required

- `plans/closure/tui-editor-helix-parity/003-status.md` containing: the bound
  and disclosure mechanism used; per-motion evidence including the cross-line
  and `t`/`T` landing tests; the non-vacuity injections proving the suite rejects
  a current-line-confined implementation and a before-target landing; the
  adversarial bound test result; the terminal-panel non-regression evidence;
  exact verification commands with local versus hosted labels; explicit
  not-applicable statements; and the registry update with the unblock audit.

## 16. Handoff notes

- **Hazard:** the two easiest defects here are (a) confining the search to the
  current line, because that is the vi behavior most implementations default to,
  and (b) making `t`/`T` land *before* the target. Both are spec-level, and both
  pass a naive test suite. The §10 non-vacuity injections exist specifically to
  catch them.
- **Hazard:** `f`/`t` capture the next key as **data**. If this leaks into the
  prefix machine's leaf resolution, a following printable character can be
  swallowed and composer typing breaks.
- **Hazard:** `Alt-.` must be a safe no-op with no recorded motion. An
  unconditional `Alt-.` binding would swallow a composer keystroke for no gain.
- Terminal key encoding for multi-byte targets may deliver decomposed units in
  some terminals; verify what is actually received rather than assuming.
- Build jobs are capped (`CARGO_BUILD_JOBS=2`); broad sweeps run under the
  nextest `ci` profile.
