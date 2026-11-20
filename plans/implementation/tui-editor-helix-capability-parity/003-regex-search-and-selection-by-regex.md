# TUI Editor Helix Capability Parity Milestone 003 — Regex Search and Selection-by-Regex

Status: conditionally closed
Repository baseline: `7642b9e381d9c91d650fa375be2a7aa0aae9bcd8`

Source roadmap:

- `plans/subsystems/tui-editor-helix-capability-parity-roadmap.md#milestone-003--regex-search-and-selection-by-regex`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#29-system-invariants`

Applicable ADRs:

- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`

Primary class: capability

## 1. Objective

Make the editor's search a **regex** search, and add the four gestures that
follow from it: `?` (reverse), `*` and `Alt-*` (seed the search from the
selection), `s` (select the matches inside the selections) and `Alt-s` (split on
newlines). The scan stays bounded and a match becomes a **span**, not a start
offset, so it can be selected and highlighted exactly.

## 2. Why this milestone is ready

No hard dependency. The regex machinery already exists and is closed:

- `compile_selection_regex` compiles under `regex::RegexBuilder` with both
  `size_limit` and `dfa_size_limit` set to `MAX_REGEX_PROGRAM_BYTES` (1 MiB), so
  a pathological program is refused at compile time rather than run.
- `bounded_regex_window` bounds a per-member scan by the 32 KiB line budget and
  the whole set by `MAX_SET_VERB_READ_BYTES` (256 KiB), and truncation is
  disclosed rather than hidden.
- `S` (`SplitRegex`), `K` (`KeepRegex`) and `Alt-K` (`RemoveRegex`) already route
  a pattern through the `:` command line into that machinery, which proves the
  command line is an adequate prompt and needs no new input mode.
- The search line, its match list, `adopt_search`, `step_search`, `has_search`
  and the `n`/`N` arms are closed and bounded.

So this milestone replaces one scanner and adds four verbs; it introduces no new
prompt surface, no new dependency, and no new authority.

## 3. Current implementation evidence

At the baseline:

- `find_matches(snapshot, needle)` uses `str::match_indices`, i.e. a **literal
  substring** scan. It reads a bounded window (`MAX_EDITOR_SEARCH_BYTES`, 2 MiB),
  stops at `MAX_EDITOR_SEARCH_MATCHES` (2 000), and returns
  `SearchOutcome { matches: Vec<usize>, truncated, bounded }` — **start offsets
  only**.
- `TuiDocumentPresentation::search_matches` is `Vec<usize>`. It is consumed by
  `first_match_index`, `next_match_index`, `previous_match_index` (binary search
  over offsets), `adopt_search`, `step_search`, `has_search`, and one renderer
  count.
- `/` seeds the search line from the word under the cursor and runs the scan on
  every keystroke. `n`/`N` are claimed **only while a search exists**, so an `n`
  with no search still reaches the composer.
- `?`, `*`, `Alt-*`, `s` and `Alt-s` are absent from the chord table. `/` is
  claimed, so `?` and `*` are unbound bare characters that fall through to the
  composer; `Alt-*` is inert.
- `MAX_EDITOR_SEARCH_LINE` is 128 bytes, so a search needle is a query and never
  document text.
- The key authority guard reports **2 / 74 / 8** with no shadowed arm.

## 4. Invariants that must not regress

- **A pattern that will not compile is a notice, not a panic and not an empty
  result.** The existing `compile_selection_regex` message shape is the model.
- **Every scan is bounded twice** — per window and by match count — and reaching
  either bound is disclosed (`truncated` / `bounded`), never silent.
- **The search line stays bounded** by `MAX_EDITOR_SEARCH_LINE`.
- **`n`/`N` stay claimed only while a search exists**, so they do not eat a
  composer keystroke.
- **No second text buffer**; matches are offsets into the live replica.
- **One owner per chord**, no shadowed arm.
- **`s`, `S`, `K` and `Alt-K` cannot disagree about regex compilation**, because
  there is one compile function.

## 5. Scope

### In scope

- `SearchOutcome.matches` becomes a span collection, and every consumer is
  updated: the index helpers, `adopt_search`, `step_search`, `has_search`, the
  renderer's count, and any test that asserted offsets.
- `/` compiles a regex and scans with it; an invalid pattern is a notice.
- `?` opens the search line seeded with the last accepted pattern and adopts the
  **previous** match.
- `*` seeds from the selection, `regex::escape`d and wrapped in `\b`; `Alt-*`
  seeds from the selection **verbatim** as a regex.
- `s` selects every regex match inside each selection member (the sibling of `S`,
  which splits into the gaps).
- `Alt-s` splits each member on newlines.
- `docs/tui.md` and the keymap box updated in the same change.

### Explicitly out of scope

- A search **history** or a `/` register. Registers are M007; the search register
  arrives with them.
- Incremental "search as you type" semantics beyond what exists today: the scan
  still runs per keystroke under the same bounds.
- Multi-line regex anchors across a bounded window boundary. A window is a
  window; the plan must state that `^`/`$` bind to line boundaries within the
  scanned window and that the disclosure covers the rest.
- Making the file picker or any other picker fuzzy.
- `grep`-style workspace-wide search. That is `space /` in Helix and is a
  different surface with its own authority question.

## 6. Required production changes

### Core/domain

None. `regex` is already a dependency of the root package and is already used by
the selection verbs.

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

None. The scan is synchronous over the snapshot, as it is today.

### Frontend or operator surface

- `src/tui/editor.rs`: `SearchOutcome` gains spans; `find_matches` becomes a
  regex scan that reuses `compile_selection_regex`; the index helpers binary
  search on a span's start; a new `HelixVerb` for `s`; a helper for `Alt-s`.
- `src/tui/document_session.rs`: `search_matches` becomes the span collection;
  `adopt_search`, `step_search`, `has_search` follow.
- `src/tui/app/render.rs`: the search count reads the new collection's length.
- `src/tui/commands/editor.rs`: the `?`, `*`, `Alt-*`, `s` and `Alt-s` arms, and
  the direction state `?` needs.

### Security and authorization

No change. `s` is a selection verb, not a mutation, so it is gated by the
existing `grows_selection` rule like `v`, `x` and the text objects.

### Documentation and static guards

- `docs/tui.md`: `/` is described as a regex search; `?`, `*`, `Alt-*`, `s`,
  `Alt-s` added; the "not bound" list loses any entry naming them.
- Both guard figures move with the chords in the same change.

## 7. Ordered work packages

### Work package A — Matches become spans

Intent: make a match carry its extent, so it can be selected and highlighted.

Required changes: `SearchOutcome.matches` and `search_matches` hold spans; the
three index helpers binary search on the span start and must remain correct for a
span list that is still ascending and non-overlapping. The renderer's count and
every existing test that asserted an offset are updated.

Acceptance evidence: the existing `/` and `n`/`N` behaviour is unchanged for a
literal pattern that contains no metacharacters; a test asserts the span extent
for a match longer than one byte; the index helpers' wrap-at-either-end behaviour
is asserted after the type change.

### Work package B — `/` becomes a regex scan

Intent: one scanner, one compile function, one disclosure path.

Required changes: `find_matches` compiles through `compile_selection_regex` and
scans the bounded window for spans. An invalid pattern returns a `SearchOutcome`
that the caller turns into a notice, and the notice names the pattern and the
compiler's message, matching the existing selection-verb wording.

**Zero-width matches are not search matches.** A pattern like `x*` can match
zero bytes, and a zero-width match cannot be selected, cannot be highlighted, and
would let `n` loop on one offset. The scan therefore skips them, and the plan
records this as a stated rule rather than an accident.

Acceptance evidence: `foo.bar` matches `fooXbar`; `foo.bar` no longer matches a
literal `foo.bar`; an invalid pattern such as `[` produces a notice and no
matches; a pattern with a zero-width match produces no zero-width entries; a
pattern exceeding the program-size limit is refused at compile with the reason.

### Work package C — `?` reverse search

Intent: search backwards, seeded with the last accepted pattern.

Required changes: a direction on the presentation, or a distinct adopt path that
takes the previous match; the search line opens pre-filled with the last accepted
pattern, which is the seed the reader expects. `?` must not need a second
direction field if `step_search`'s existing `forward: bool` can be reused — prefer
reusing it.

Acceptance evidence: `/foo` then `?` adopts the previous `foo` without retyping
the pattern; `?` with no previous pattern opens an empty search line rather than
doing nothing silently.

### Work package D — `*` and `Alt-*` seed from the selection

Intent: use the current selection as the pattern.

Required changes: read the primary member's text, then:

- `*` applies `regex::escape` and wraps the result in `\b…\b`, so `foo.bar`
  selects the literal `foo.bar` as a whole word rather than matching `fooXbar`.
- `Alt-*` uses the selection **verbatim** as a regex, so `foo.bar` matches
  `fooXbar`.

With no selection, both are a no-op that reports nothing — a reader with a bare
cursor asked to search for nothing.

Acceptance evidence: `*` on a selection containing `.` matches the literal text
only; `Alt-*` on the same selection matches the wildcard; a selection spanning a
newline is refused with a reason rather than silently truncated to its first
line, because a multi-line pattern is a different gesture.

### Work package E — `s` select-matches and `Alt-s` split-on-newlines

Intent: the two selection-shaping regex verbs.

Required changes: `s` prompts for a pattern through the `:` command line, exactly
as `S`/`K`/`Alt-K` do, and replaces each member with the matches found inside it.
`Alt-s` splits each member on newline boundaries with no regex at all.

Acceptance evidence: `s` on a selection containing three matches yields three
members; `s` with a pattern that matches nothing leaves the selection unchanged
and reports nothing; `s` is refused on a read-only attachment (it grows the
selection) with a notice; `Alt-s` on a two-line member yields two members whose
bounds are the lines' content.

### Work package F — Guard and documentation

Intent: keep the binding set coherent in one change.

Required changes: update both guard figures; document the regex change and the
zero-width rule; update the keymap box.

Acceptance evidence: the key authority guard passes with the new count and no
shadowed arm; the doc-drift guard passes in both directions.

## 8. Failure, cancellation, restart, and contention semantics

- **Partial failure.** A regex that does not compile produces a notice and an
  empty match set; the search line keeps the text so the reader can fix it.
- **Duplicate delivery.** None; synchronous local scan.
- **Restart.** No state.
- **Cancellation races.** None; no async work added.
- **Stale generations.** The scan reads the current snapshot.
- **Bound reached.** `truncated` (match count) and `bounded` (byte window) are
  already carried and already surface as `search_partial`; the notice text must
  name which bound was hit, not merely that something was partial.

## 9. Compatibility and migration

No storage, protocol, or configuration migration.

**The compatibility surface is the search grammar itself, and this is a
behaviour change rather than an addition.**

- **D1 — `/` becomes regex, unconditionally.** Today `/foo.bar` matches the
  literal four characters `foo.bar`; after this milestone the `.` is a wildcard.
  Three candidates were considered. *(a)* Always regex. *(b)* Literal unless the
  pattern is wrapped in a delimiter. *(c)* A toggle between the two modes.
  **Recommendation: (a).** (b) and (c) are a second grammar: the reader would have
  to know which mode they are in to predict what a pattern means, and the
  editor's own precedent — `S`, `K` and `Alt-K` — is already always-regex. The
  cost is a one-time surprise for a pattern containing a metacharacter, and it is
  paid once, in the documentation, rather than on every search.
  The plan must state the change in `docs/tui.md` in the same change as the
  scanner.

- **Removal criteria for the old behaviour:** none. There is no compatibility
  window and no fallback path; a literal search is spelled with an escaped
  pattern, which the regex engine already supports.

## 10. Required tests

### Focused unit tests

- The regex scanner: a literal pattern unchanged, a wildcard pattern, an escaped
  pattern, a zero-width pattern, an invalid pattern, and a pattern at the byte
  and match-count bounds.
- The span index helpers after the type change, including wrap at both ends.
- `regex::escape` behaviour for `*`, including a selection containing `.`, `[`,
  `\` and a newline.

### Integration tests

- One dispatch-path test per chord through `editor_buffer_key`.
- `/` then `n` then `N` walks the regex matches and wraps.
- `?` adopts the previous match without retyping.
- `s` yields one member per match; `Alt-s` yields one member per line.

### Restart and recovery tests

None. No durable state; the closure record must say so rather than omit it.

### Contention and cancellation tests

- `s` over a set whose total exceeds `MAX_SET_VERB_READ_BYTES` discloses the
  truncation rather than returning a short list that looks complete.

### Security and negative tests

- `check_tui_editor_text_authority.py` passes with no allowlist edit.
- A pattern exceeding `MAX_REGEX_PROGRAM_BYTES` is refused at compile with the
  reason, and the scan does not run.
- A pattern that would produce more than `MAX_EDITOR_SEARCH_MATCHES` sets
  `truncated` and the notice names the count bound.

### Migration and compatibility tests

- The literal-to-regex behaviour change is asserted explicitly: a test that fails
  on the old scanner and passes on the new one, so the change is recorded rather
  than discovered.
- The doc-drift guard as the keymap compatibility check.

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

`verify.sh quick` runs no tests. The `cargo test` lines are the test evidence;
`quick` is the guard evidence.

## 12. Documentation updates

- `docs/tui.md`: `/` described as a regex search with the escape rule stated; `?`,
  `*`, `Alt-*`, `s`, `Alt-s` in the tables; the zero-width rule stated; the
  "not bound" list loses any entry naming them.
- The `space ?` keymap box follows.
- The code comment on `find_matches` states the zero-width rule and why, so a
  later reader does not "fix" the skip.

## 13. Acceptance criteria

- `/foo.bar` matches `fooXbar`, and `/foo\.bar` matches the literal text.
- An invalid pattern produces a notice naming the pattern and the compiler's
  message, and leaves no matches.
- `?` adopts the previous match, seeded with the last accepted pattern.
- `*` on a selection containing a metacharacter matches the literal selection;
  `Alt-*` on the same selection treats it as a regex.
- `s` replaces each member with one member per match; `Alt-s` splits on lines.
- Every bound that is reached is named in the notice.
- The key authority guard reports no shadowed arm; the doc-drift guard passes in
  both directions.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- Changing `search_matches` to spans cannot be done without a second source of
  truth for the cursor's position in the match list. Report the shape rather than
  adding a parallel index.
- The zero-width rule cannot be implemented without changing `step_search`'s
  contract in a way that also affects `n`/`N` for normal patterns.
- A regex feature (backreferences, lookaround) is expected by a user and the
  `regex` crate does not support it. That is a dependency decision, not a plan
  detail.
- The work expands into a search history, a `/` register, or workspace-wide
  search.
- The literal-to-regex change is judged unacceptable in review. That inverts D1
  and needs an owner decision, not a fallback bolted on.

## 15. Closure evidence required

- The key authority guard's summary with the new count and no shadowed arm.
- One dispatch-path test per chord, named individually.
- The literal-to-regex change asserted by a test that fails on the old scanner.
- The bound disclosures: program size, byte window, match count, each named.
- The `s` and `Alt-s` set-shape evidence.
- A statement that no migration, protocol variant, crate, or new authority was
  added.
- The commands actually run, labelled local or CI, with unrun items named.

## 16. Handoff notes

- **`search_matches` has six consumers.** Change them in one pass; a half-migrated
  list is a binary search over the wrong type.
- **The renderer reads the match count.** `src/tui/app/render.rs` will fail to
  compile rather than silently render a wrong count — that is the good direction,
  so do not paper over it with a second length field.
- **`s` is a selection verb, not a search verb.** It is gated by
  `grows_selection` and refused on a read-only attachment, exactly as `v` and `x`
  are.
- **`*` must escape.** The selection is literal text; using it as a pattern
  unescaped is the defect `Alt-*` exists to expose deliberately.
- **Serial test constraint.** No environment mutation is introduced; nextest
  profile `ci` applies.
- **Preserve the user's working tree.** No generated asset is touched.
