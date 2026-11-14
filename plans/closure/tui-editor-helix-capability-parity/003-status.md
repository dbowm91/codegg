# TUI Editor Helix Capability Parity Milestone 003 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/tui-editor-helix-capability-parity/003-regex-search-and-selection-by-regex.md`

Source subsystem roadmap:

- `plans/subsystems/tui-editor-helix-capability-parity-roadmap.md#milestone-003--regex-search-and-selection-by-regex`

Repository baseline reviewed: `7642b9e38` (`feat-auto-20261109-f8abad0d`), plus
uncommitted working-tree work

Implementation commits or pull requests:

- **None.** This milestone exists only as uncommitted working-tree work on
  `feat-auto-20261109-f8abad0d`, on top of HEAD `7642b9e38`. There is no commit,
  branch or pull request carrying it, and the same working tree also carries
  M001, M002 and M004–M009 of this roadmap plus the whole
  `tui-modularity-and-render-invariants` line. No statement below is
  attributable to a SHA.

## 1. Executive finding

The capability is delivered. `/` is a **regex** search that shares one compile
function with the selection verbs; a match is a **span** rather than a start
offset, so it can be selected and highlighted exactly; and the four gestures
that follow from it are bound — `?` (reverse), `*` and `Alt-*` (seed the search
from the selection), `s` (select the matches inside the selections) and `Alt-s`
(split on newlines).

**One definition of "compiles".** `find_matches`
(`src/tui/editor.rs:1033`) compiles through the same private
`compile_selection_regex` (`src/tui/editor.rs:4253`) that `S`/`s`/`K`/`Alt-K`
use, so the search and the selection verbs cannot disagree about what compiles,
about the refusal wording, or about the program-size bound:

```rust
regex::RegexBuilder::new(pattern)
    .size_limit(MAX_REGEX_PROGRAM_BYTES)
    .dfa_size_limit(MAX_REGEX_PROGRAM_BYTES)
    .build()
    .map_err(|error| format!("`{pattern}` is not a valid regex: {error}"))
```

A pattern that does not compile is a **reason** carried on the outcome, not a
panic and not an empty result — the two answers are different and only one of
them is the reader's to fix. `SearchOutcome.matches` and
`TuiDocumentPresentation::search_matches` are `Vec<Range<usize>>`
(`src/tui/document_session.rs:271`), and the index helpers binary-search a
span's start, so `n`/`N` and the renderer read one ascending, non-overlapping
list.

**The five chords are bound, and the fallthrough they used to be is closed.**
`?` at `src/tui/commands/editor.rs:5462`, `*` at `:5480`, `Alt-*` at `:5484`,
`s` at `:5200` and `Alt-s` at `:5209`. Before this milestone `?` and `*` were
unbound bare characters that fell through to the composer and typed themselves
into the prompt box; `Alt-*` was inert. All five are now claimed by the buffer.

The three non-vacuity proofs the plan asks for are present and they distinguish
the cases that matter:

1. **"Matched nothing" is not "refused".**
   `an_invalid_pattern_carries_the_compilers_reason_and_no_matches` asserts an
   invalid pattern yields `pattern_error: Some(...)` with an empty set, and a
   pattern that compiles and matches nothing yields `pattern_error: None` with
   an empty set — the second case is the one that tells the reader nothing,
   which is the correct answer.
2. **An invalid regex leaves the line fixable.** The refusal carries the
   compiler's message, and the search line keeps its text, so the reader can
   edit rather than retype.
3. **A multi-byte match is a whole-character span.**
   `a_match_span_covers_whole_characters_across_a_multibyte_pair` asserts
   `1..7` over `"a漢字b\n"`, that reading the span yields `"漢字"`, and that
   both edges satisfy `is_byte_boundary`.

**Two latent defects were fixed as a side effect of sharing one function, and
neither was caught by an existing test.** Both repairs are pinned by **new**
tests rather than by edits to an older one
(`a_zero_width_match_is_not_a_search_match`,
`selecting_matches_past_the_read_budget_refuses_rather_than_truncating`), which
is why the defects survived until the type change forced a review of the
function. The span migration did separately update existing assertions that
read start offsets, as the plan required; that is a different change and is not
counted here as a defect repair.

1. `MAX_SET_VERB_READ_BYTES` was **accumulated and never checked**, while its
   own documentation claimed the gesture was bounded by it. `bounded_regex_window`
   added to a `budget` that no caller compared against the limit, so a set of
   members large enough to exceed the ceiling produced a short result that read
   as complete. `split_selection` now refuses —
   `if budget > MAX_SET_VERB_READ_BYTES { return Err(...) }`
   (`src/tui/editor.rs:4117`) — with the reason naming the budget, and
   `selecting_matches_past_the_read_budget_refuses_rather_than_truncating`
   pins it. The choice of refusal over truncation is deliberate and documented:
   a member that was never scanned contributes no matches, and a set that looks
   complete is the defect the budget exists to prevent.
2. **Zero-width matches produced backwards one-character members.**
   `split_selection` computed a member's head from `found.end() - 1`, so a
   zero-byte match could place the head before the anchor or inside a
   multi-byte character. Both scanners now skip a match whose
   `start() == end()` — `find_matches` at `editor.rs:1063-1065`,
   `split_selection` in the same shape — and the head is derived through
   `last_char_start`, which walks back to a character boundary. The rule is
   stated as a rule in `find_matches`' doc comment so a later reader does not
   "fix" the skip.

**One plan gap, recorded rather than papered over.** The plan's §16 says
"`search_matches` has six consumers" and tells the implementer to change them
in one pass. That list was **incomplete**: it missed the `/`-match highlight
painter, which read start offsets and now lives at
`src/tui/components/editor.rs:1714-1747` (the plan cites `1682-1712`, which was
correct at baseline before the code above it grew). The painter reads spans,
clips each to the visible row with `saturating_sub` and `clamp`, and uses
`partition_point` on `span.start` to find the first span on a line rather than
walking every retained match for every visible row. It was migrated to spans
(one function plus three literals) rather than left as an uncompilable tree.
This is a **plan documentation defect**, not a missing deliverable: the type
change made it a compile error, which is the good direction, and the migration
is correct. But a reader trusting §16 would have been surprised, and the plan's
own instruction ("a half-migrated list is a binary search over the wrong type")
was aimed at exactly the risk it then under-described.

**One behaviour change, and it is a real narrowing.** Bare `?` on the editor
route is now the reverse search, so general help is no longer reachable there
in Normal mode. It remains on `Space` (`space ?` is the editor's own catalogue),
and `?` still opens general help elsewhere — the composer route is unchanged.
`docs/tui.md` states both scopes: `:562` records that help is unaffected on the
composer side, and `:2091-2093` states that in the buffer `?` is the reverse
search and the general help stays on `Space`. The scoping is documented rather
than left for a reader to discover by pressing the key.

The status was conditional for two reasons, neither of them the capability, and
the gate half is now discharged: `scripts/verify.sh quick` and `cargo fmt
--check` were not run by this record's author, and the comment-ratchet invariant
did not hold — `check_comment_budget.py` was red with three violations on this
milestone's edit path (§10). The 2026-11-13 `scripts/verify.sh full` run passed
on this tree — 17,377 workspace tests and 8,441 feature-pass tests, 0 failures,
`cargo fmt --check --all` clean, clippy `-D warnings` clean — and `full` calls
`quick`, which calls `run_guards()`, so both gate conditions are met and the
editor-path ceilings are re-banked in `docs/comment-budget.toml` (§10). One
condition remains open and is a plan-inventory edit, not a gate: §11 condition 3,
correcting the plan's §16 consumer list.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| `SearchOutcome.matches` becomes a span collection | `find_matches` returns `matches: Vec<Range<usize>>` (`editor.rs:1033-1075`); `find_matches_returns_every_span_in_order` | pass | Spans are absolute UTF-8 byte offsets into the replica. |
| Every consumer is migrated in one pass | `search_matches: Vec<Range<usize>>` (`document_session.rs:271`); `adopt_search`, `step_search`, `has_search` (`:590-625`); `first/last/next/previous_match_index`; the renderer count; the `/` highlight painter | pass | Seven consumers, not six — see §1 and §10. |
| The index helpers binary-search a span's start and still wrap at both ends | `stepping_walks_matches_and_wraps_at_both_ends`; `search_lands_on_the_match_the_cursor_is_already_on` | pass | The list is ascending and non-overlapping, which is what makes the binary search valid. |
| `/` compiles a regex and scans with it | `slash_searches_a_regex_and_n_and_n_cap_it`: `/a.b` over `"axb a.b\n"` matches `axb` (offset 0) **and** `a.b` (offset 4), and `n`/`N` walk both and wrap | pass | **This test fails on the old substring scanner**, which is the plan's §10 "record the change rather than discover it" requirement. |
| …and `foo.bar` no longer matches the literal `foo.bar` | `a_dot_in_the_pattern_is_a_wildcard_and_an_escaped_dot_is_not` | pass | `.` is a wildcard; `\.` is the literal spelling. |
| An invalid pattern is a notice naming the pattern and the compiler's message, with no matches | `an_invalid_pattern_carries_the_compilers_reason_and_no_matches` | pass | The line keeps its text, so the pattern stays fixable. |
| A pattern with a zero-width match produces no zero-width entries | `a_zero_width_match_is_not_a_search_match`: `x*` over `"abc\n"` yields none; `b*` yields exactly `1..2` | pass | The one match that covers a byte survives. |
| A pattern past the program-size limit is refused at compile, with the reason, and the scan does not run | `a_program_past_the_size_limit_is_refused_before_the_scan_runs` — `(a{400}){400}` reports "size limit", matches empty | pass | `MAX_REGEX_PROGRAM_BYTES` is 1 MiB on both `size_limit` and `dfa_size_limit`. |
| A match longer than one byte carries its extent | `a_match_span_covers_whole_characters_across_a_multibyte_pair` | pass | `1..7` reading `"漢字"`, both edges on character boundaries. |
| The scan is bounded twice and each bound is disclosed | `MAX_EDITOR_SEARCH_BYTES` 2 MiB cut at a character boundary (`bounded`), `MAX_EDITOR_SEARCH_MATCHES` 2 000 (`truncated`), and `search_partial()` names which bound was hit | pass | The notice text distinguishes "the first N matches" from "within the 2 MiB scan window" and names both when both fired. |
| The search line stays bounded | `MAX_EDITOR_SEARCH_LINE` 128; `the_search_line_is_bounded_at_the_keystroke` | pass | A query, never document text. |
| `?` opens the line seeded with the accepted pattern and adopts the previous match | `question_searches_backwards_without_retyping_the_pattern` | pass | One strict step first (the cursor usually sits on the match `/` accepted), then inclusive re-adoption while refining. |
| `?` with no previous pattern opens an empty line rather than doing nothing | `question_with_no_previous_pattern_opens_an_empty_line` | pass | |
| `*` escapes and word-wraps the selection; `Alt-*` uses it verbatim | `star_searches_for_the_literal_selection_and_alt_star_does_not` — a selection containing `.` matches the literal characters under `*` and the wildcard under `Alt-*` | pass | `regex::escape` plus `\b…\b` for the first; the second is the deliberate unescaped spelling. |
| `*` / `Alt-*` with no selection is a no-op that reports nothing | `selection_search_pattern` returns `Ok(None)` and the caller returns without a notice (`commands/editor.rs:4195-4198`) | pass | A reader with a bare cursor asked to search for nothing. |
| A selection spanning a newline is refused with a reason | `selection_search_pattern` returns `Err`; asserted at `editor.rs:8407` ("a multi-line pattern is a different gesture") | pass | Not truncated to its first line. |
| `s` prompts through the `:` line and replaces each member with one member per match | `s` opens `select-regex ` exactly as `S`/`K`/`Alt-K` do; `select_matches_replaces_one_member_with_one_per_match`; `split_makes_one_member_per_match` | pass | `s` and `S` share `split_selection`, so they cannot disagree about a match. |
| `s` with a pattern that matches nothing leaves the set alone and reports nothing | `split_whose_pattern_matches_nothing_leaves_the_set_alone` | pass | |
| `s` is refused on a read-only attachment with a notice | `s_is_a_selection_verb_and_is_refused_on_a_read_only_document` | pass | Gated by `grows_selection`, exactly as `v` and `x` are. |
| `Alt-s` splits each member on newlines, with no regex and no prompt | `alt_s_splits_the_selection_on_newlines_through_the_key_path` | pass | One member per line, bounded by each line's content. |
| `s` over a set past `MAX_SET_VERB_READ_BYTES` discloses rather than truncating silently | `selecting_matches_past_the_read_budget_refuses_rather_than_truncating`; the refusal names the budget | pass | `MAX_SET_VERB_READ_BYTES` = `MAX_FRAME_SOURCE_BYTES` = 256 KiB. |
| `n`/`N` stay claimed only while a search exists | The arms are guarded on `session.presentation().has_search()` (`commands/editor.rs:5489`, `:5497`) | pass | An `n` with no search still reaches the composer as the start of a new session name. |
| `check_tui_editor_text_authority.py` passes with no allowlist edit | run: pass; one informational borrow note at `src/tui/components/editor.rs:213` | pass | The note is a borrowed `DocumentSnapshot` field, not a retained copy. |
| The key-authority guard reports no shadowed arm | run: "2 guard-block chord(s), **90** Normal-mode chord(s) [9 digits claimed by a count arm: 123456789], 8 Insert-mode chord(s), no shadowed arm" | pass | 90 is the **current tree**; at M003's close the figure was **89** (84 + this milestone's five chords). M004's count arm adds the one. See §4. |
| The doc-drift guard passes in both directions | run: "52 leaf gestures and **98** chords are documented, and every key declared unbound is unbound" | pass | 93 → 98 in this milestone's change. M004 does not move it: the count grammar is one documented row, not nine. |
| `docs/tui.md` gains the five keys and loses any "not bound" entry naming them; the keymap box follows | `docs/tui.md:1698-1701` (the `s`/`Alt-s`/`S`/`K` table), `:1732-1757` ("The regex keys"), `:2302-2336` ("a regex, compiled by the same bounded builder", the `Alt-*` row, the word-boundary rule), `:2091-2093` (the `?` narrowing); `EDITOR_KEYMAP` carries `"/ / ?"` | pass | Documentation moved in the same change as the arms, which is the plan's §16 requirement. |
| No second text buffer; matches are offsets into the live replica | text-authority guard passes; both scanners read through `snapshot.read_bytes` | pass | No retained bytes. |
| No migration, protocol variant, crate or new authority | `git status` shows no `codegg-core`/`codegg-document`/`codegg-protocol` change attributable here, and no manifest change; `regex` was already a root dependency | pass | Stated as §15 requires, not omitted. |
| The literal-to-regex change is asserted by a test that fails on the old scanner | `slash_searches_a_regex_and_n_and_n_cap_it` | pass | See the first row of this group. |

## 3. Production implementation evidence

- `src/tui/editor.rs` — `SearchOutcome::matches` is a span list; `find_matches`
  is a bounded regex scan through `compile_selection_regex`; the zero-width
  rule is stated in its doc comment; `compile_selection_regex` is the single
  compiler; `split_selection` is the single `S`/`s` resolution and it refuses
  past the read budget; `last_char_start` keeps a member's head on a character
  boundary.
- `src/tui/document_session.rs` — `search_matches: Vec<Range<usize>>`,
  `search_cursor`, and the `adopt_search` / `step_search` / `has_search` trio
  following the type change.
- `src/tui/commands/editor.rs` — the five arms. `?` seeds from the search line
  and takes one strict step before re-adopting inclusively; `*`/`Alt-*` resolve
  through `selection_search_pattern`, which returns a reason for a multi-line
  selection and `None` for no selection; `s` opens the `:` line with
  `select-regex `; `Alt-s` applies `HelixVerb::SplitSelectionOnNewlines`
  directly.
- `src/tui/components/editor.rs:1714-1747` — the `/`-match highlight painter,
  migrated to spans: `partition_point` on `span.start`, per-span clipping to
  the visible row, and one `from..to` per span. **This is the consumer the
  plan's §16 list omitted.**
- `src/tui/app/render.rs` — the search count reads the new collection's length.
  The plan predicted this would fail to compile rather than silently render a
  wrong count; it did, and it was fixed in the same pass.
- `docs/tui.md` and `EDITOR_KEYMAP` — updated in the same change as the arms.

Nothing in the plan's "explicitly out of scope" list was implemented: no search
history, no `/` register, no workspace-wide search, no fuzzy picker, and no
change to the per-keystroke scan semantics.

## 4. Verification executed

### Commands run

All commands were run locally on the host named in this record, in
`/Users/davidbowman/projects/codegg/.worktrees/feat-auto-20261109-f8abad0d`, on
2026-11-11 between 19:44 and 19:58.

```bash
python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_tui_editor_text_authority.py
python3 scripts/check_tui_editor_doc_drift.py
python3 scripts/check_tui_editor_leaf_authority.py

cargo test -p codegg --lib tui::editor
cargo test -p codegg --lib tui::commands::editor
cargo test --test tui_render -- tui_editor_cost
cargo test --test tui_render
python3 scripts/check_comment_budget.py
```

### Results

| Command | Result |
|---|---|
| `check_tui_editor_key_authority.py` | **pass** — "2 guard-block chord(s), 90 Normal-mode chord(s) [9 digits claimed by a count arm: 123456789], 8 Insert-mode chord(s), no shadowed arm" |
| `check_tui_editor_text_authority.py` | **pass** — "no retained text buffer and no direct filesystem reads in the editor path"; one informational note, a borrowed `DocumentSnapshot` field at `src/tui/components/editor.rs:213` |
| `check_tui_editor_doc_drift.py` | **pass** — "52 leaf gestures and 98 chords are documented, and every key declared unbound is unbound" |
| `check_tui_editor_leaf_authority.py` | **pass** — "6 leaf table(s), 52 leaf glyph(s), 13 prefix binding(s), 5 unavailable leaf reason(s), no duplicate and no unreachable leaf" |
| `cargo test -p codegg --lib tui::editor` | **pass** — 311 passed, 0 failed, 0 ignored, 7763 filtered out, finished in 5.77s |
| `cargo test -p codegg --lib tui::commands::editor` | **pass** — 278 passed, 0 failed, 0 ignored, 7796 filtered out, finished in 2.89s |
| `cargo test --test tui_render -- tui_editor_cost` | **pass** — 13 passed, 0 failed, 1208 filtered out |
| `cargo test --test tui_render` (whole family) | **pass** — 1221 passed, 0 failed, 0 filtered out, finished in 16.65s |
| `check_comment_budget.py` | **fail at the time of writing; superseded 2026-11-13 — see the correction log.** 48 violations tree-wide, five on this milestone's edit path: `src/tui/editor.rs` 3458 comment lines against 2926, `src/tui/commands/editor.rs` 2604 against 2275 (ratio 0.2322 against 0.2270), `src/tui/components/editor.rs` 853 against 840 (ratio 0.2214 against 0.2202), `src/tui/document_session.rs` 403 against 351, and `src/tui/editor_highlight.rs` 313 against 281 (another session's M005 work). Corpus-wide: 137,275 lines against 134,015, 1,220,763 words against 1,188,348, ratio 0.1354 against 0.1344. All five ceilings are now banked and the guard passed inside the 2026-11-13 `full` |

Two honesty notes on how these were produced:

- **The Normal-mode chord figure is 90 at this tree state, not 89.** The plan's
  §3 baseline is 74; M001 added seven, M002 added three and this milestone adds
  five, which is **89**. M004's single count arm is the ninetieth, and the guard
  names it separately ("9 digits claimed by a count arm") because one arm claims
  nine characters. After M005–M008 the same tree reads 90 with no commit
  separating the milestones, so the per-milestone figure of 89 is arithmetic
  from the plan plus the arm-by-arm reading in `handle_normal_key`; it cannot be
  re-measured, and this record does not pretend otherwise.
- **The 98 documented chords are this milestone's figure and still are**, which
  is the one place where a per-milestone number *can* be checked: M004's count
  scheme is documented as a grammar rather than as nine chord rows, so the
  doc-drift reading did not move after M003.
- **A transient build failure preceded these runs.** The first
  `cargo test --test tui_render` at 19:37 failed to compile the library with
  `E0599` at `src/tui/editor_highlight.rs:572` (`SYNTAX_SET.metadata()` with
  syntect's `metadata` feature not enabled). `Cargo.toml` was changed by
  another session at 19:43:42 to enable that feature, and every result above
  comes from the successful rebuild. Nothing in this milestone touches
  `editor_highlight.rs` or the syntect dependency. Several invocations also
  queued on the shared build-directory lock held by sibling sessions in the
  same worktree; the reported finishes are real runs after the lock cleared.
- **The tree moved during the review.** `src/tui/editor.rs` was modified at
  19:46:04 by a sibling session, before the binaries these numbers come from
  were built (`codegg-7f071e724f49550c` at 19:56,
  `tui_render-c222fb5313649f59` at 19:52). Every source file on this
  milestone's path predates those binaries, so the figures describe this
  milestone's artifacts at those times, not a standing guarantee.

### Not run

| Command | Why |
|---|---|
| `cargo fmt --check`, `bash scripts/verify.sh quick` | Not run at the time of writing: `quick` runs the routine guards plus a workspace check over a tree other sessions were editing, and `check_comment_budget.py` alone was red for files this milestone did not touch, so a red result would not have been attributable here. **Discharged 2026-11-13** — the `scripts/verify.sh full` run passed on this tree (`full` → `quick` → `run_guards()`, whose first line is `cargo fmt --check --all`). |
| `cargo clippy --workspace --all-targets -- -D warnings` | Not run at the time of writing: `verify.sh full` was out of scope for that review. **Discharged 2026-11-13** — clippy `-D warnings` is part of `run_full()` and was clean in that run. |

## 5. Invariant review

- **One compile function.** `find_matches` and the selection verbs call the
  same `compile_selection_regex`; the message shape and the program-size bound
  are shared, so `s`/`S`/`K`/`Alt-K` and `/` cannot disagree.
- **Every scan is bounded twice and each bound is disclosed.**
  `MAX_EDITOR_SEARCH_BYTES` (2 MiB, cut at a character boundary) and
  `MAX_EDITOR_SEARCH_MATCHES` (2 000); `search_partial()` names which bound was
  hit, and names both when both fired.
- **The search line stays bounded** at 128 bytes, so a needle is a query and
  never document text.
- **`n`/`N` stay claimed only while a search exists.** The guard is
  `has_search()` on both arms.
- **No second text buffer.** Both scanners read through the live snapshot and
  retain nothing; the text-authority guard passes with no allowlist edit.
- **One owner per chord.** The key-authority guard reports no shadowed arm, and
  `s` is claimed with `m == none` while `Alt-s` is `m == alt`, so they are
  distinct events rather than a shadowing pair.
- **Positions, never text.** Every match is a byte span into the replica; no
  verb retains a `DocumentSnapshot` or a copy of the text.
- **A refusal is not an empty result.** `pattern_error` distinguishes them in
  the type, not in a convention.

## 6. Failure and recovery review

- **Partial failure.** A regex that does not compile produces a notice and an
  empty set, and the line keeps its text so the reader can fix it. A set past
  the read budget **refuses** rather than returning a short list, because a
  short list that reads as complete is the silent failure the budget exists to
  prevent.
- **Duplicate delivery.** Not applicable; synchronous local scan.
- **Restart.** **No state**, stated explicitly as the plan's §8 requires rather
  than omitted: the search line and match list are presentation state and do
  not survive a restart.
- **Cancellation races.** None; no async work is added and no request is issued.
- **Stale generations.** Both scanners read the current snapshot;
  `try_snapshot()` cannot return a stale one.
- **Bound reached.** Program size at compile; byte window and match count at
  run time. Each is named in the notice, which is what §8 asks ("the notice
  text must name which bound was hit, not merely that something was partial").
- **Zero-width matches.** Skipped by rule at both scanners, with the reason in
  the code, because a zero-byte match cannot be selected, highlighted or
  stepped without making `n` re-report one offset.

## 7. Migration and compatibility review

**No storage, protocol or configuration migration.** The compatibility surface
is the **search grammar**, and this is a behaviour change rather than an
addition:

1. **D1 — `/` is regex, unconditionally.** Before, `/foo.bar` matched the
   literal four characters; now `.` is a wildcard. The plan considered a
   delimiter-scoped literal mode and a toggle, and chose always-regex because
   the alternatives are a second grammar the reader would have to track, and
   because `S`/`K`/`Alt-K` were already always-regex. There is no compatibility
   window and no fallback; a literal search is spelled with an escaped pattern.
   Recorded in `docs/tui.md` in the same change as the scanner, and asserted by
   a test that fails on the old scanner.
2. **The keymap.** `?`, `*`, `Alt-*`, `s` and `Alt-s` stop reaching the
   composer on the editor route. For `?` this is visible: general help is no
   longer reachable from the buffer in Normal mode. It remains on `Space`, and
   `?` still opens help from the composer.

Rollback is deleting the arms and reverting the span type; nothing is
persisted.

## 8. Security review

No authorization surface changes. `s` is a selection verb, not a mutation, and
is gated by the existing `grows_selection` rule, exercised rather than assumed
(`s_is_a_selection_verb_and_is_refused_on_a_read_only_document`). Every write
path this milestone can reach still goes through `apply_local`. Three
denial-of-service bounds are load-bearing and all three are enforced rather than
documented: the 1 MiB compiled-program limit (a pathological pattern is refused
at compile, before it can run), the 2 MiB scan window and 2 000-match cap, and
the 256 KiB set read budget, which now **refuses** instead of truncating. A
pattern's compilation error is surfaced as a reader-facing notice; it carries no
document bytes, only the pattern the reader typed and the compiler's message.

## 9. Documentation and operations

- `docs/tui.md` — `/` described as a regex search with the escape rule stated;
  `?`, `*`, `Alt-*`, `s`, `Alt-s` in the tables; the zero-width rule; the
  read-only refusal list; and the explicit statement that bare `?` in the buffer
  is the reverse search while `space ?` is the catalogue and general help is on
  `Space`.
- `src/tui/components/editor.rs` `EDITOR_KEYMAP` — the `"/ / ?"` row.
- `src/tui/editor.rs` — the zero-width rule and the "do not fix the skip"
  instruction on `find_matches`; the refusal-over-truncation rule on
  `split_selection`.
- Guards: key authority, text authority, doc drift and leaf authority all pass,
  with their figures moved in the same change as the chords.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | **The plan's §16 "six consumers" list was incomplete.** It omitted the `/`-match highlight painter, now `src/tui/components/editor.rs:1714-1747`, which read start offsets and had to be migrated to spans. | A reader following §16 would have hit an uncompilable tree at a consumer the plan did not name. The migration is correct and complete, so the delivered code is sound; the defect is in the plan's inventory. | When this roadmap is revised, correct §16 to name the painter (and describe it as seven consumers). Optionally add a note that the renderer's count and the painter are the two read-only consumers. |
| low | **Discharged 2026-11-13.** The comment corpus ratchet did **not** hold at this record's close: `check_comment_budget.py` reported 48 violations tree-wide, five on this milestone's edit path, with `src/tui/editor.rs` 532 comment lines over its ceiling, `src/tui/commands/editor.rs` 329 over (and over its ratio ceiling), `src/tui/components/editor.rs` 13 over (and over its ratio ceiling), and `src/tui/document_session.rs` 52 over. The ceilings are now banked in `docs/comment-budget.toml` (`editor.rs` 3693, `commands/editor.rs` 3110, `components/editor.rs` 902, `document_session.rs` 717, `editor_highlight.rs` 313) and the guard passed inside the 2026-11-13 `scripts/verify.sh full`. | The scanner and span rules are explained inline where they apply, and the ratchet is the mechanism that forces that prose to be paid for deliberately. | None. One tree-wide violation has since appeared in `src/tui/components/dialogs/keybind.rs` from the corrective plan's WP D deletion, outside this path and owned by that work package's manifest bank. |
| low | **Discharged 2026-11-13.** `cargo fmt --check` and `scripts/verify.sh quick` were not run by this record's author. | §11's own guard evidence was the two editor guards plus the whole render family, not the routine gate. | None: the 2026-11-13 `scripts/verify.sh full` ran `quick` and `run_guards()` (which contains `cargo fmt --check --all`) on this tree, green. |
| low | The `space ?` keymap box is a condensed list and does not carry every new spelling; `docs/tui.md` is the complete document. | A reader who learns the verbs from the in-editor box may not discover one of the new keys; there is no guard on the box. | Add the missing rows, or record in `src/tui/components/editor.rs` why the box is condensed. Carried forward from M001/M002's records. |

No medium or high finding. No migration, protocol variant, crate or new
authority was added.

## 11. Roadmap disposition

**Milestone conditionally closed.** The regex scanner, the span type and its
seven consumers, the five new chords, the two latent-defect repairs and the
documentation are complete and verified. The conditions are:

- **Condition.** Before M004 is cited as depending on a closed M003:
  1. `cargo fmt --check` and `scripts/verify.sh quick` are green; — **met
     2026-11-13** by the `scripts/verify.sh full` run (17,377 workspace tests +
     8,441 feature-pass tests, 0 failures; formatting and clippy clean).
  2. `python3 scripts/check_comment_budget.py` is green, or the four editor-path
     ceilings are re-banked with a recorded reason (§10); — **met 2026-11-13 by
     the second arm**: `editor.rs` 3693, `commands/editor.rs` 3110,
     `components/editor.rs` 902 and `document_session.rs` 717 are banked, and
     the guard passed inside that `full`.
  3. the plan's §16 consumer inventory is corrected when the roadmap is revised.
     — **not met, and still the milestone's own residue**: the plan still names
     six consumers and omits the `/`-match highlight painter. That is an edit to
     `plans/implementation/**`, owned by the roadmap revision rather than by
     this record.
- **Risk.** Low on capability. The family is 1221/1221 at this record's close
  (1,237 in the 2026-11-13 `full`), 311 filtered library tests in `tui::editor`
  and 278 in `tui::commands::editor` pass, the four editor guards pass, and the
  bound disclosures are exercised rather than assumed. The one user-visible
  narrowing (`?`) is documented in both scopes. The former process risk — a
  tree-wide gate red in a tree carrying several uncommitted milestones — is
  discharged; what remains is the plan-inventory correction and the commit.
- **Exact future evidence.** Satisfied for conditions 1–2 by the 2026-11-13
  `scripts/verify.sh full` run and the re-banked ceilings. Still owed for
  condition 3: the corrected §16 inventory in the plan, plus the commit that
  carries this plan (WP 0).

M004 may proceed on the span type and the shared compiler. It must not assume
the §16 inventory is complete.

## 12. Registry updates

- Mark roadmap M003 **conditionally closed** and link this record.
- Record that `/` is regex by D1, that the per-milestone Normal-mode chord count
  is 89 (the current tree reads 90 with M004's count arm), and that the
  documented chord figure is 98.
- Record the `?` narrowing in the roadmap's keymap notes so M005–M009 do not
  assume general help is reachable from the buffer.
- Do not mark M004 ready on the strength of this record alone.

This record does not edit `plans/registry.md`, the source plan's `Status:`
line, or the roadmap's status table. Those are owned by a separate sweep; this
record's author was scoped to `plans/closure/**` only, and every status claim
above is a recommendation for that sweep rather than a change it made.

## Correction log (2026-11-13)

- **§1, the status paragraph** — "`scripts/verify.sh quick` and `cargo fmt
  --check` were not run … `check_comment_budget.py` is red with three violations
  on this milestone's edit path" → the gate half is discharged by the 2026-11-13
  `scripts/verify.sh full` (17,377 + 8,441 tests at 0 failures, formatting and
  clippy clean) and by the re-banked editor-path ceilings; the one remaining
  condition is the plan's §16 inventory.
- **§4, the comment-budget result row** — "**fail** — 48 violations …" → marked
  superseded: all five cited ceilings are banked and the guard passed inside the
  2026-11-13 `full`.
- **§4, `Not run`** — "`cargo fmt --check`, `bash scripts/verify.sh quick` …
  Not run" and "clippy … Not run" → both discharged by the 2026-11-13
  `scripts/verify.sh full` (`full` → `quick` → `run_guards()`, whose first line
  is `cargo fmt --check --all`, plus the clippy step).
- **§10, low (comment ratchet)** — "The comment corpus ratchet does **not**
  hold … 48 violations" → discharged 2026-11-13 by the bank and the `full` run.
- **§10, low (`fmt`/`quick` not run)** — discharged by the same run.
- **§11, conditions 1–2** — each marked met with its discharge named.
  **Condition 3 remains open**: the plan's §16 still names six consumers and
  omits the `/`-match highlight painter (`src/tui/components/editor.rs`), which
  is an edit to `plans/implementation/**` owned by the roadmap revision.
- **§11, risk** — "`quick` and the comment ratchet are tree-wide gates and both
  are red" → discharged; the remaining risk is the uncorrected plan inventory.
- **§11, exact future evidence** — the transcripts and re-banked ceilings exist;
  the corrected §16 inventory and the commit (WP 0) are still owed.
- **Run figures left as written** — §2 and §4 report "90 Normal-mode chord(s)",
  "98 chords" and 1221 render tests from the runs recorded in §4. They are
  unchanged historical measurements, not conditions: the guards now report 92
  Normal-mode chords / 100 documented chords and the render family 1,237 tests.
