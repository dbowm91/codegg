# TUI Editor Helix Capability Parity Milestone 005 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/tui-editor-helix-capability-parity/005-comment-toggling.md`

Source subsystem roadmap:

- `plans/subsystems/tui-editor-helix-capability-parity-roadmap.md#milestone-005--comment-toggling`

Repository baseline reviewed: `7642b9e38` (`feat-auto-20261109-f8abad0d`), plus
uncommitted working-tree work — 228 changed or untracked paths at the time of
review

Implementation commits or pull requests:

- **None.** This milestone exists only as uncommitted working-tree work on
  `feat-auto-20261109-f8abad0d`, on top of HEAD `7642b9e38`. There is no commit,
  branch or pull request carrying it, and the same working tree also carries
  M001–M004 and M006–M009 of this roadmap (M006 was being written while this
  record was being written; §4 records the two transient compile breaks that
  caused). No statement below is attributable to a SHA.

**Line numbers are from the working tree at 21:10 on 2026-11-11, not from
HEAD.** Other lanes moved them while this record was written — the comment-toggle
tests in `src/tui/commands/editor.rs` shifted by roughly 290 lines between the
first and the last read of the file — so the **function and test names are the
authoritative reference** and the numbers are a convenience for a reader of this
tree.

## 1. Executive finding

The capability is delivered. `space c` and `space C` toggle comments over the
covered lines in the edited buffer's **own grammar's** tokens, as one undoable
transaction per gesture, with the selection preserved so a second press is the
inverse of the first. The two `Unavailable` rows are gone from the space table,
from the guarded record, and from `docs/tui.md`, which now documents the working
verbs, the geometry, the recognition rule and the refusal.

The four load-bearing properties the plan names are implemented as structure
rather than convention:

1. **One selector, not a second table.** `comment_tokens_for`
   (`src/tui/editor_highlight.rs:570-583`) runs `syntax_for(path)` — the same
   function the highlighter uses — then
   `SYNTAX_SET.metadata().metadata_for_scope(&[syntax.scope])` and reads
   `line_comment()` / `block_comment()`, i.e. the grammar's own declared
   `TM_COMMENT_START`/`TM_COMMENT_END`. "A file that colours correctly toggles
   correctly" is therefore true by construction rather than by two tables
   agreeing. The two strings are the grammar's own, trailing space included
   (`"// "`, not `"//"`), which is what makes removal the exact inverse of
   insertion rather than a normalisation (`editor_highlight.rs:510-516`).
2. **One transaction, one history entry.** `toggle_comments`
   (`src/tui/editor.rs:4905-4982`) builds the whole `TextTransaction` and hands
   it to `build_edit` once. The dispatch-path test
   `space_c_comments_a_range_as_one_undo_step`
   (`src/tui/commands/editor.rs:13761`) asserts a three-line toggle is
   `undo_depth == 1` and that one `u` takes all of it back.
3. **The selection survives the gesture.** `submit_edit` clears every selection
   member on an accepted edit, so a toggle that did not re-publish its members
   would leave a second press acting on the cursor's line alone — the inverse
   property would hold for one line and fail for a range. `toggle_comments`
   returns `HelixVerbOutcome::EditAndSelect` with the members shifted by the
   same pre-edit offsets the cursor uses (`editor.rs:4955-4981`), and
   `a_toggle_keeps_the_selection_it_acted_on` (`editor.rs:9493`) asserts
   one member in, one member out, moved by the inserted tokens.
4. **An unknown syntax is a stated refusal, not a guess.**
   `comment_toggle_outcome` (`src/tui/commands/editor.rs:5870-5909`) returns
   `HelixVerbOutcome::NothingToDo` with a reason that names the extension
   (`comment_extension`, `:5900-5909`), and `apply_helix` reports every
   `NothingToDo` once at the point the outcome is read (`:5855-5863`), so no
   caller can forget it. `None` from the lookup is deliberately one answer for
   three cases — no grammar, a grammar with no token, an empty metadata set —
   because the outcome is the same and the message already says which.

**The `metadata` feature is now load-bearing, and that constrains a future
revert.** `Cargo.toml:184` declares
`syntect = { version = "5", features = ["metadata"] }`; `syntect` 5.3.0's
`default` set is `["default-onig"]`, which does **not** include `metadata`
(`$CARGO_HOME/registry/src/…/syntect-5.3.0/Cargo.toml`, `[features]`). The call
at `src/tui/editor_highlight.rs:572` is `SYNTAX_SET.metadata()`, so reverting
`Cargo.toml` **alone** fails the build with `E0599` at that line. This is not
inference: the same failure was observed at 19:37 during M004's review (recorded
in `004-status.md` §4) when `editor_highlight.rs` was already calling
`metadata()` and the manifest change had not landed. **Rolling this milestone
back is the whole change — feature, lookup, verbs, geometry, leaves, guard and
docs — not the manifest line alone.**

**The measured cost, recorded as measurements rather than guarantees.** As
reported by the implementing session: the `syntect` rlib grows by
**+3,346,576 B (+27.0 %)**; an equal-work probe binary grows by
**+343,040 B (+20.7 %)**; a clean `cargo build -p syntect` moved 2.17 s → 1.68 s,
**within noise, so no build-time delta is claimed**. This record independently
corroborated the rlib figure with a throwaway project outside the repository
(no repo edit): `syntect` 5.3.0 built with default features is **12,487,064 B**
and with `metadata` added is **15,899,168 B**, a delta of **+3,412,104 B
(+27.33 %)**. The ratio reproduces to within 0.3 percentage points and the
absolute figures to within ~1 %; the small residual is explained by the
workspace's `[profile.dev] strip = "debuginfo"` (`Cargo.toml:574-577`), which
applies to the workspace artifact and not to the throwaway project. The
**probe-binary** figure was **not** reproduced: the implementing session's probe
is not in the tree, so that number rests on that session's record alone.

**The leaf-authority guard gained the record it previously lacked, and its
non-vacuity is proven by injection.** At baseline the guard had **no**
`UNAVAILABLE_SPACE_LEAVES` set at all (`git show HEAD:scripts/…` contains zero
occurrences of the name) and `SPACE_LEAVES` carried **seven** `Unavailable`
rows: `F`, `b`, `j`, `D`, `c`, `C`, `'`. M005 removed `c` and `C` and added
rule 6, which compares the table against the record **in both directions**
(`compare_unavailable_rows`, `scripts/check_tui_editor_leaf_authority.py`): an
unrecorded reason is "a working verb that kept its apology, or a new gap", and a
recorded reason the table no longer states is "the capability works, so delete
the record". The recorded set is **5**; `--self-test` runs **13 cases**,
including both rule-6 directions and the reworded-reason case. Non-vacuity was
reproduced here in process (no file edited): dropping `j` from the record makes
the real-tree run pass with **4** reasons, and re-adding the two comment rows on
top of that produces **exactly 2** rule-6 violations — the two rows this
milestone turned into working verbs.

**The guard was red in this tree when this record was written, and the red was
M006's, not M005's; M006 has since discharged it.** `space j` is now
`PrefixAction::Effect(EditorPendingEffect::OpenJumplist)`
(`src/tui/editor.rs:750-752`) and the record no longer lists `j`, so the real
run passes: "6 leaf table(s), 52 leaf glyph(s), 15 prefix binding(s), 4
unavailable leaf reason(s), no duplicate and no unreachable leaf" (measured
2026-11-13). At the time of writing it reported one violation for the stale `j`
row. §10 keeps that history with its discharge.

**The render ratchet did not move at this record's close (the figures below are
that tree's; the guard now reads 4 / 0 / 0).**
`check_tui_render_invariant_baseline.py`
reported **61 advertised-key, 2495 contrast and 1 hint-truncation** entries at
the time of writing, every one reproducing and none added unbanked;
`docs/tui-render-invariant-baseline.toml`
is untracked in this tree with mtime 16:48, predating every file this milestone
touched (18:28–18:45). One keymap row was added to `EDITOR_KEYMAP`
(`src/tui/components/editor.rs:272`), and it added no unbanked entry. The
**Rust** side of that lane was separately red in this tree — two
`tui_render -- invariants` tests, both the render lane's own half-finished
rewrite and neither mentioning the editor or a comment (§10) — and the lane has
since settled both: the 2026-11-13 `full` ran the render corpus at 1,237 tests
with 0 failures, and the Python guard now reads **4 / 0 / 0** after the
tui-modularity line's re-basing. That disagreement between the Python guard and
the Rust family is the reason this record named it, and it is closed.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| The token lookup reads the grammar's own declared tokens through the same selector the highlighter trusts | `comment_tokens_for` → `syntax_for` → `metadata_for_scope` (`editor_highlight.rs:570-583`); `syntax_for` (`:503-508`) | pass | One selector; no extension→token table exists anywhere. |
| `.rs` declares `// ` and a `/* */` pair | `rust_declares_a_line_token_and_a_block_pair` (`editor_highlight.rs:982-994`); direct probe: `line=Some("// ") block=Some(("/*","*/"))` | pass | Probe used the same accessor from a throwaway binary (§4). |
| `.py` declares `# ` and no pair | `python_declares_its_own_line_token_and_no_block_pair` (`:996`); probe agrees | pass | The pair-less refusal path is reachable from a real grammar. |
| `.md` — the plan's required token-lookup case | **No test.** Probe: `line=None block=Some(("<!-- "," -->"))` | **partial** | The plan's parenthetical ("line token only") is wrong about the shape; the grammar is block-only. §10. |
| An extension with no grammar, and an empty path, answer `None` | `a_grammar_with_no_comment_token_answers_none` (`:1007`) covers `.txt`, `.zzz`, `""` | pass | `.txt` has a grammar and no token; `.zzz` has no grammar. |
| `space c` prefers the line token and falls back to the pair; `space C` forces the pair | `CommentTokens::style` (`:542-563`); `the_line_style_is_preferred_and_the_block_style_is_forced` (`:1018`) | pass | The **fallback** branch (`line: None`, `block: Some`) has no test — `.md`/`.css`/`.html` are that branch. §10. |
| The line token goes at each covered line's first non-whitespace byte | `line_comment_edits` uses `first_non_whitespace` (`editor.rs:4989-5034`); `a_line_toggle_comments_the_cursor_line_after_its_indentation` (`:9476`) | pass | Indented line keeps its indentation; the token is inserted after it. |
| The covered range is the selected lines, or the cursor's line | `toggle_comments` (`editor.rs:4910-4922`); `a_line_toggle_covers_every_line_the_selection_spans` | pass | The cursor's **column** never decides anything. |
| The block pair wraps the range: start before its first non-whitespace byte, end after its last | `block_comment_edits` (`editor.rs:5041-5110`); `a_block_toggle_wraps_the_range_and_returns_the_same_bytes` (`:9568`) | pass | Text between the tokens is untouched. |
| Blank lines are skipped rather than given a token | `line_comment_edits` skips `content_end <= start`; `a_blank_line_in_the_range_is_skipped_rather_than_commented` (`:9524`) | pass | A blank-only range refuses (`a_range_of_blank_lines_is_refused_with_a_reason`). |
| Un-comment requires **every** non-blank line to be commented; a mixed range comments | `uncomment = targets.iter().all(…)` (`editor.rs:5021`); `a_mixed_range_comments_rather_than_half_uncommenting` (`:9514`) | pass | Stated rule, asserted. |
| A toggle is its own inverse over several shapes | `toggling_twice_restores_the_exact_bytes` (`editor.rs:9544`): single line, nested 3-line block, mixed tab/space indent, range with a blank line, already-`// ` text | pass | The plan's acceptance property, asserted over five shapes. |
| A block toggle whose interior contains the end token still round-trips | `a_block_toggle_does_not_inspect_the_interior_for_the_end_token` (`:9583`): `a */ b` → `/*a */ b*/` → `a */ b` | pass | The plan's stop condition has a stated rule and a test. |
| One transaction per gesture, one undo entry | dispatch test `space_c_comments_a_range_as_one_undo_step` (`commands/editor.rs:13761`), `undo_depth == 1`, one `u` restores | pass | §6 of the plan. |
| The selection survives so a second press un-toggles | `EditAndSelect` with shifted members (`editor.rs:4963-4981`); `a_toggle_keeps_the_selection_it_acted_on`; `space_c_toggles_a_line_comment_and_the_second_press_restores_it` (`commands/editor.rs:13740`) | pass | The inverse property at the dispatch level. |
| `space c` and `space C` are the working leaves | `PrefixAction::ToggleComments { block }` (`editor.rs:768-773`); dispatch `commands/editor.rs:4823-4825`; `apply_helix` `:5844` | pass | Both leaves go through the same `comment_toggle_outcome`. |
| An unknown syntax is refused, names the extension, and leaves the document byte-identical | `a_buffer_with_no_comment_token_is_refused_and_names_the_extension` (`commands/editor.rs:13809`) for `.txt` and `.zzz`: notice contains the extension and "no comment syntax", text unchanged, `undo_depth == 0` | pass | Two shapes because they fail for different reasons. |
| `space C` on a pair-less grammar refuses | Implementation: `tokens.style(block)` → `None` → notice at `:5887-5891`; token level: `python_declares_its_own_line_token_and_no_block_pair` asserts `style(true) == None` | **partial** | No dispatch-path test asserts that notice. §10. |
| Read-only attachment is refused visibly, with no history entry | `space_c_on_a_read_only_attachment_is_refused_visibly` (`commands/editor.rs:13836`): `EditorNotice::ReadOnly`, text unchanged, `undo_depth == 0`; `mutates_buffer` includes `ToggleComments` (`:6008`) | pass | Gated at the verb **and** at the shared funnel. |
| Past `MAX_DOCUMENT_EDITS` the toggle is refused, naming the limit, not truncated | `toggle_comments` (`editor.rs:4931-4937`); `a_toggle_past_one_transactions_limit_is_refused_not_truncated` (`:9600`) | pass | Limit constant is 256 (`crates/codegg-protocol/src/document.rs:7`, unmodified). |
| The leaf guard passes with two fewer `Unavailable` rows | Real run 2026-11-13: **pass**, "4 unavailable leaf reason(s)", `j` absent from the record; at HEAD the rows were 7 and the set did not exist | pass | At this record's close the run reported one rule-6 violation for M006's landed `space j`; M006 removed the row. §10. |
| Rule 6 compares both directions | `compare_unavailable_rows` (three arms); `--self-test` 13 cases including "a reason the guard does not record" and "a recorded row that stopped stating a reason" | pass | Re-adding `c`/`C` produces exactly 2 violations. |
| The doc-drift guard passes in both directions | Run: "52 leaf gestures and 98 chords are documented, and every key declared unbound is unbound" | pass | Both figures unchanged from M004's close. |
| `check_tui_editor_text_authority.py` passes with **no** allowlist edit | Run: pass, one informational borrow note at `components/editor.rs:213`; `git status` shows the guard **unmodified** | pass | No retained text buffer. |
| The key-authority guard still reports one owner per chord | Run: "2 guard-block chord(s), 90 Normal-mode chord(s) [9 digits claimed by a count arm: 123456789], 8 Insert-mode chord(s), no shadowed arm" | pass | 90 is M004's figure; M005 adds no chord. Its diff is M004's digit claim only. |
| The prefix machine is not widened | `check_tui_editor_prefix_state.py`: "one pending-prefix field (pending_prefix), typed as EditorPrefix, no legacy mechanism left" | pass | `space c` is a leaf glyph, not a new prefix. |
| No migration, protocol variant, crate, or new authority | `Cargo.lock` **unchanged** (0 lines); `crates/codegg-protocol/src/document.rs` unchanged; `crates/codegg-document/**` unchanged; neither modified protocol/core file's diff mentions "comment" | pass | A feature on an existing dependency, not a crate. |
| The `metadata` feature's cost is measured and recorded | Implementing session's figures in §1; independent rlib re-measurement in §1 and §4 | pass | Probe-binary figure not reproduced. §10. |
| The render ratchet does not move | `check_tui_render_invariant_baseline.py`: 61 / 2495 / 1, none unbanked | pass for M005's contribution | Baseline file predates this milestone's edits. The family's own Rust tests are separately red for the render lane's rewrite — §10. |

## 3. Production implementation evidence

- `Cargo.toml:180-184` — `syntect` gains `metadata`, with the comment that says
  why it is a feature rather than a crate. This is the only manifest change
  attributable to the milestone; `Cargo.lock` is byte-unchanged.
- `src/tui/editor_highlight.rs:510-583` — `CommentTokens` (the grammar's two
  tokens), `CommentStyle` (the style chosen for one gesture), `CommentTokens::style`
  (the line-prefers-pair preference and its refusal), and `comment_tokens_for`
  (the single selector). The doc comments state that `syntax_for` is the selector
  and why a hand-maintained table was rejected.
- `src/tui/editor.rs:4890-5110` — `toggle_comments` (coverage, one transaction,
  the shifted selection set), `line_comment_edits` (first-non-whitespace
  insertion, the all-commented recognition rule, mixed ranges comment),
  `block_comment_edits` (the wrap, the wrap's own recognition, the
  pair-is-whole-range case), and `MAX_DOCUMENT_EDITS` refusal.
- `src/tui/editor.rs:768-773` — the two `SPACE_LEAVES` rows move from
  `Unavailable` to `PrefixAction::ToggleComments { block }`.
- `src/tui/commands/editor.rs:4823-4825`, `:5844`, `:5870-5909` —
  `PrefixAction::ToggleComments` → `HelixVerb::ToggleComments` →
  `comment_toggle_outcome`, which resolves the attachment's path, the tokens, the
  style and the refusal; `comment_extension` builds the name the notice carries.
- `src/tui/commands/editor.rs:6008` — `mutates_buffer` gains
  `ToggleComments`, so a read-only reader is told the gesture is unavailable
  before the edit funnel is reached.
- `scripts/check_tui_editor_leaf_authority.py` — rule 6, the
  `UNAVAILABLE_SPACE_LEAVES` record, its parser, the both-ways comparison, three
  rule-6 self-test cases, and the rule's own doc comment naming M005 as the
  change that shrank the set.
- `docs/tui.md:2079-2082`, `:2110-2150` — the two rows become working verbs, and
  the section states the geometry, the skip-blank rule, the recognition rule, the
  one-undo-step rule, the block toggle's non-parsing limit, the refusal, and the
  `Ctrl-c` divergence with its reason.
- `architecture/tui.md:7846` — the stale "unavailable" row now describes the
  toggle and the refusal, with the extension named. (Corrected by the parent
  before this record; verified here, not edited here.)
- `src/tui/components/editor.rs:253`, `:272` — the `space ?` box gains
  `("space c / C", "toggle line / block comment")`.

Nothing in the plan's "explicitly out of scope" list was implemented: `Ctrl-c`
is still `InputAction::ForceQuit` plus copy-selection; `space Alt-c` does not
exist (`PrefixLeaf.glyph` is still an unmodified `char`); there is no
insert-at-point verb, no formatter interaction, no per-language override, and no
doc-comment convention (`///` versus `//`) — the grammar's `TM_COMMENT_START` is
the only rule.

## 4. Verification executed

### Commands run

Every command below was run locally on the host named in this record, in
`/Users/davidbowman/projects/codegg/.worktrees/feat-auto-20261109-f8abad0d`, on
2026-11-11 between 20:33 and 21:0x, on the shared target directory. Commands
that queued behind sibling sessions were re-run; the reported times are the runs
that produced the results.

```bash
python3 scripts/check_tui_editor_leaf_authority.py
python3 scripts/check_tui_editor_leaf_authority.py --self-test
python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_tui_editor_text_authority.py
python3 scripts/check_tui_editor_doc_drift.py
python3 scripts/check_tui_editor_prefix_state.py
python3 scripts/check_tui_render_invariant_baseline.py
python3 scripts/check_comment_budget.py

cargo test -p codegg --lib tui::editor_highlight
cargo test -p codegg --lib tui::editor::tests
cargo test -p codegg --lib tui::commands::editor
cargo test --test tui_render -- invariants
```

Two measurements were taken outside the repository and touched no file in it:

```bash
# grammar tokens, through the same accessor the feature uses
rustc --edition 2021 -O /tmp/probe_comment_tokens.rs \
  -L dependency=target/debug/deps \
  --extern syntect=$(ls target/debug/deps/libsyntect-*.rlib | head -1) \
  -o /tmp/probe_comment_tokens && /tmp/probe_comment_tokens

# rlib cost, with and without `metadata`, in throwaway target directories
cd /tmp/syntect-probe && cargo build --offline          # default features
cd /tmp/syntect-probe && cargo build --offline          # + features = ["metadata"]
```

### Results

| Command | Result |
|---|---|
| `check_tui_editor_leaf_authority.py --self-test` | **pass** — "self-test passed: 13 case(s)", including both rule-6 directions and the reworded-reason case |
| `check_tui_editor_leaf_authority.py` | **fail at the time of writing; superseded 2026-11-13 — see the correction log.** 1 violation: `[rule 6] the recorded 'Unavailable' row 'j' no longer states a reason in SPACE_LEAVES`. M006's `space j` landed at `editor.rs:750`; M005's surface is clean (§1). With `j` removed in process: **pass** — "6 leaf table(s), 52 leaf glyph(s), 13 prefix binding(s), 4 unavailable leaf reason(s), no duplicate and no unreachable leaf". With `c`/`C` re-added on top: **exactly 2** rule-6 violations. The 2026-11-13 run passes with 4 reasons and 15 prefix bindings |
| `check_tui_editor_key_authority.py` | **pass** — "2 guard-block chord(s), 90 Normal-mode chord(s) [9 digits claimed by a count arm: 123456789], 8 Insert-mode chord(s), no shadowed arm" |
| `check_tui_editor_text_authority.py` | **pass** — "no retained text buffer and no direct filesystem reads in the editor path"; one informational borrow note at `src/tui/components/editor.rs:213` |
| `check_tui_editor_doc_drift.py` | **pass** — "52 leaf gestures and 98 chords are documented, and every key declared unbound is unbound" |
| `check_tui_editor_prefix_state.py` | **pass** — "one pending-prefix field (pending_prefix), typed as EditorPrefix, no legacy mechanism left" |
| `check_tui_render_invariant_baseline.py` | **pass** — "61 advertised-key, 2495 contrast and 1 hint-truncation entries recorded; every one reproduces and none was added unbanked" |
| `check_comment_budget.py` | **fail at the time of writing; superseded 2026-11-13 — see the correction log.** "52 comment-budget violation(s)"; corpus 137,555 comment lines against 134,015, 1,223,618 comment words against 1,188,348, ratio 0.1355 against 0.1344. Four of them were on this milestone's path: `src/tui/editor.rs` 3497 against 2926, `src/tui/commands/editor.rs` 2651 against 2275 (ratio 0.2336 against 0.2270), `src/tui/editor_highlight.rs` 313 against 281, `src/tui/components/editor.rs` 883 against 840 (ratio 0.2225 against 0.2202). None of those four ceilings was re-banked in that tree; all four are now banked (313 / 3693 / 3110 / 902) |
| `cargo test -p codegg --lib tui::editor_highlight` | **pass** — 17 passed, 0 failed, 0 ignored, 8090 filtered out, finished in 0.15 s |
| `cargo test -p codegg --lib tui::editor::tests` | **pass** — 229 passed, 0 failed, 0 ignored, 7878 filtered out, finished in 5.89 s (a pre-20:50 binary also gave 225 passed / 0 failed / 7869 filtered out in 1.27 s; the 229 figure is the later one and the one to cite) |
| `cargo test -p codegg --lib tui::commands::editor` | **fail** — 284 passed, **1 failed**, 0 ignored, 7822 filtered out, finished in 2.78 s. The failure is `tui::commands::editor::tests::a_motion_records_nothing_and_each_named_jump_records_where_it_started`, a **jumplist** test at `commands/editor.rs:13512` (`left: 8, right: 4`). It is not a comment-toggle test and not M005's code |
| `cargo test --test tui_render -- invariants` | **fail** — 29 passed, **2 failed**, 0 ignored, 1193 filtered out, finished in 10.48 s. The filter selected **31** tests, not the 28 this review was told to expect, because the render lane added members while the record was being written — the count is itself evidence the tree moved. Both failures are the **render-invariants lane's** in-flight rewrite, not this milestone: `invariants::vocabulary::the_recorded_hint_of_every_surface_is_what_it_renders` compares a 13-entry sorted `RECORDED` table (still carrying `ProjectInit`) against a 28-entry surface-ordered measurement; `invariants::baseline::the_recorded_i4_baseline_is_exactly_what_the_harness_measures` reports the manifest recording lowercase `enter` where the harness now measures `Enter`, with totals 59 against 61. Neither mentions a comment, the editor, or the `space ?` box. §10 |
| grammar-token probe | `rs: line=Some("// ") block=Some(("/*","*/"))`; `py: line=Some("# ") block=None`; `md: line=None block=Some(("<!-- "," -->"))`; `txt: line=None block=None`; `zzz: no grammar`; also `sh`/`yaml`: `# `, no pair; `css`: no line, `("/*","*/")`; `html`: no line, `("<!-- "," -->")`; **`toml`: no grammar**; `ts`: no grammar; the default set is **75 syntaxes** and contains no TOML grammar at all |
| syntect rlib measurement | default features **12,487,064 B**; with `metadata` **15,899,168 B**; delta **+3,412,104 B (+27.33 %)**. Workspace artifact on disk is 15,725,728 B (stripped, `[profile.dev] strip = "debuginfo"`), consistent with the implementing session's 15,725,728 B and +3,346,576 B (+27.0 %) |

**Two transient compile breaks preceded these runs, both in M006's work, both
in a file M005 also edits.** Recording them because they explain the ordering
and because the same file is on this milestone's path:

- at ~20:34 the lib test target failed with `E0308` at
  `src/tui/commands/editor.rs:3979` — `note_jumplist_jump(state, Some(before))`
  where `before` was already `Option<usize>`; the sibling session changed the
  call to `note_jumplist_jump(state, before)` at 20:34:53 and the next build
  compiled;
- at ~20:45 it failed with `E0063` at `src/tui/components/editor.rs:3643` —
  `missing field 'jumplist' in initializer of Options`; the sibling added
  `jumplist: Jumplist::default()` at 20:46:22.

Neither break is in the comment-toggle code, and neither symbol exists at HEAD.
The final results above come from builds made between 20:52 and 21:07, after the
tree's last source edit (20:50:59), i.e. later than every file this milestone
touched (18:28–18:45). Several invocations also queued on the shared
build-directory lock held by sibling sessions — one attempt sat at 0.6 s of CPU
over eight minutes without acquiring it — and the results above are the runs that
actually executed.

**On the `tui::commands::editor` failure.** In the earlier run (binary built
20:46) **two** jumplist tests failed:
`a_motion_records_nothing_and_each_named_jump_records_where_it_started` and
`an_entry_the_shorter_document_no_longer_contains_is_refused`. The sibling's
20:50:59 edit fixed the second; the first remains, and it is not a
comment-toggle test — its own body walks `j`, `gg` and `G` through the
jumplist record. M005 neither adds nor changes any jumplist code.

### Not run

| Command | Why |
|---|---|
| `cargo fmt --check` | Not run at the time of writing: the review scope forbade workspace formatting operations, and the tree was moving. **Discharged 2026-11-13** — `cargo fmt --check --all` is the first line of `run_guards()`, and the 2026-11-13 `scripts/verify.sh full` passed on this tree. |
| `bash scripts/verify.sh quick`, `bash scripts/verify.sh full` | Not run at the time of writing — both were outside that review's rules. **Discharged 2026-11-13** by the `scripts/verify.sh full` run: `full` calls `quick`, which calls `run_guards()`, and the two reds named in the original entry (the leaf guard's `j` row and the comment budget's editor-path ceilings) were both cleared before it. §11. |
| `cargo test --test tui_render` (the whole family) | Only the `invariants` member was in scope; the full family is 1200+ tests and belongs to the session that merges the tree. |
| Reproduction of the **probe-binary** size delta (+343,040 B, +20.7 %) | The implementing session's probe is not in the tree, so there is nothing to rebuild. The rlib delta was reproduced instead. |

## 5. Invariant review

- **One undo step per toggle.** Asserted at the dispatch level (three lines,
  `undo_depth == 1`, one `u` restores). The single `TextTransaction` is
  structural, not conventional: `toggle_comments` cannot return a partial edit
  because it builds the whole edit list before submitting once.
- **A toggle is its own inverse.** `toggling_twice_restores_the_exact_bytes`
  asserts byte equality over five shapes; the removal path deletes exactly
  `token.len()` bytes at the position it inserted them
  (`line_comment_edits`, `block_comment_edits`), so a grammar whose token has a
  trailing space (`"// "`) round-trips rather than normalising to `"//"`.
- **The toggle does not rewrite content.** Every edit is a zero-width insertion
  or an exact-length deletion; no edit spans a line's interior except the two
  pair tokens. The block toggle's documented limitation (it does not parse the
  interior) is recorded in `docs/tui.md` and asserted as a round-trip in
  `a_block_toggle_does_not_inspect_the_interior_for_the_end_token`.
- **An unknown syntax is a stated refusal.** Three shapes (`no grammar`,
  `grammar with no token`, `no block pair`) all produce a notice that names the
  extension and a byte-identical document. No path guesses and none is silent.
- **No second text buffer.** `check_tui_editor_text_authority.py` passes with an
  unmodified guard; the toggle reads a `DocumentSnapshot` and hands the
  controller a transaction.
- **One owner per gesture.** `Ctrl-c` is still the exit chord and copy-selection;
  no editor binding claims it. `space c`/`space C` are leaf glyphs in the one
  `SPACE_LEAVES` namespace, and the guard reports no duplicate and no shadowed
  arm.
- **The document and the code agree in both directions.** `docs/tui.md` and
  `EDITOR_KEYMAP` describe the working verbs; the doc-drift guard passes with 52
  gestures and 98 chords, and the leaf guard's rule 6 is the mechanical form of
  the other direction.

## 6. Failure and recovery review

- **Partial failure.** The toggle is one transaction; a rejection produces
  `NothingToDo` and the document is unchanged, so there is no half-commented
  state. A refused range (all blank) and a range past the transaction bound are
  both refused rather than truncated.
- **Duplicate delivery.** Not applicable; this is a synchronous local gesture
  with no queue and no request.
- **Restart.** **No state**, stated explicitly as the plan's §8 requires rather
  than omitted: nothing here is durable, and no startup path reads it.
- **Cancellation races.** None; no async work is added and no request is issued.
  The gesture reads the current snapshot, so there is no generation to go stale.
- **Stale generations.** The toggle resolves against `session.try_snapshot()` at
  the moment of the gesture; a detached or absent document produces a stated
  refusal ("no document is open" / "the document is not attached") rather than a
  stale write.
- **Contention and release.** No lock, lease or permit is taken. The only
  shared resource is the build-directory lock, which matters to the *review*, not
  to the feature.
- **Malformed or unauthorized input.** The only input is the covered range and
  the grammar's token; both are bounded (`first_non_whitespace` is the same
  bounded walk `gs` uses — `editor.rs:5000-5011`, with a test that a 4 MiB
  indented line stays bounded). Authorization is the read-only gate, refused
  visibly.
- **Bound reached.** `MAX_DOCUMENT_EDITS` is 256; the refusal names the limit and
  the document is byte-identical.
- **Syntax-set load failure.** `SYNTAX_SET` is a `Lazy` static loaded once; the
  call site cannot fail. An empty or missing grammar is the same `None` the
  refusal path already handles.

## 7. Migration and compatibility review

**No storage, protocol or configuration migration.** `Cargo.lock` is unchanged,
so no crate, version or feature edge was added beyond the one feature on an
existing dependency; `crates/codegg-protocol/src/document.rs` (where
`MAX_DOCUMENT_EDITS` lives) and `crates/codegg-document/**` are unmodified. The
two modified protocol/core files in this tree (`codegg-protocol/src/dto.rs`,
`codegg-core/src/session/schema.rs`) contain no "comment" in their diffs and
belong to other work.

**Two compatibility surfaces.**

1. **The dependency's feature set.** Enabling `metadata` adds compiled-in
   metadata accessors and removes no API, so it is additive at the crate level.
   The cost is the compatibility statement: +27 % on the `syntect` rlib and
   +20.7 % on an equal-work probe binary, with no measurable build-time delta.
   The feature is **load-bearing**: reverting the manifest line alone is a build
   failure at `editor_highlight.rs:572`, so rollback is the whole change.
2. **The gesture.** `space c` and `space C` change from "state that the feature
   is absent" to "toggle". There is no transitional state and no removal
   criterion; the old behaviour was a statement, not a feature.

**Rollback.** Delete the two leaves' `ToggleComments` action, the toggle verbs
and helpers, the token lookup, the manifest feature, the two documentation
sections and rule 6 with its record — in one change, for the reason in 1.

## 8. Security review

No authorization surface changes. The toggle writes through the same
`submit_edit`/`apply_local` funnel every writing verb uses, and additionally
declares itself in `mutates_buffer` so a read-only attachment is refused
**before** the funnel with `EditorNotice::ReadOnly` — asserted at the dispatch
level with `undo_depth == 0`. It reaches no path, process, secret or network
surface: the only filesystem-adjacent input is the attachment's
`relative_path`, from which it reads an extension and nothing else.

The one denial-of-service shape the feature could have introduced — a selection
spanning an unbounded number of lines turned into an unbounded edit list — is
closed by `MAX_DOCUMENT_EDITS` (256) with a refusal that names the bound, and by
the bounded first-non-whitespace walk, which has its own huge-indent test. The
comment text is never parsed, so no grammar can be made to backtrack on it; the
block toggle's non-parsing limitation is a correctness note, not an attack
surface.

## 9. Documentation and operations

- `docs/tui.md` — the working verbs; the grammar-token rule and why it cannot
  disagree with the highlighter; the line/block preference; the indentation
  rule; the skip-blank rule; the recognition rule; the one-undo-step rule; the
  block toggle's non-parsing limitation; the refusal that names the extension;
  the `Ctrl-c` divergence with its reason.
- `src/tui/components/editor.rs` `EDITOR_KEYMAP` — the `space c / C` row.
- `src/tui/editor_highlight.rs` — the doc comments on `CommentTokens`,
  `CommentStyle` and `comment_tokens_for` are the operational contract: which
  selector is authoritative, why the tokens are the grammar's own strings, and
  that `None` is one answer for three cases.
- `src/tui/editor.rs` — `toggle_comments`' doc comment states the geometry, the
  skip rule, the mixed-range rule and why the selection is republished.
- `scripts/check_tui_editor_leaf_authority.py` — rule 6 and its record, with the
  both-directions rationale in the module docstring. The guard's `--self-test`
  **is** wired into `scripts/verify.sh quick` and `.github/workflows/ci.yml`
  (both run it immediately before the guard), so the record cannot silently
  drift. The key-authority guard's `--self-test` is now wired there too, which
  discharges M004's recorded condition 3.
- Operator diagnostics: the refusals are notices, not log lines; there is no new
  command, flag or recovery instruction.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | **Resolved 2026-11-13 — the leaf-authority guard was red for a reason M005 did not own.** At this record's close `space j` was `OpenJumplist` (`src/tui/editor.rs:750-752`) while `UNAVAILABLE_SPACE_LEAVES` still recorded `j`, so the real run reported one rule-6 violation; M005's own surface passed. M006 has since deleted the `j` entry: the real run now passes with **4** unavailable leaf reasons (`F`, `b`, `D`, `'`) and 15 prefix bindings. | It did fail `verify.sh quick` and CI on a milestone that did the right thing while the milestone that landed the change did not, which is why it was recorded rather than left as noise. | None; discharged by M006's change and confirmed by the 2026-11-13 run. |
| medium | **Discharged 2026-11-13 — the comment corpus ratchet did not hold at this record's close.** `check_comment_budget.py` then reported 52 violations tree-wide, four on this milestone's path (`editor.rs` 3497/2926, `commands/editor.rs` 2651/2275 and over its ratio, `editor_highlight.rs` 313/281, `components/editor.rs` 883/840 and over its ratio), and none of the four ceilings was re-banked. All four are now recorded in `docs/comment-budget.toml` (`editor.rs` 3693, `commands/editor.rs` 3110, `editor_highlight.rs` 313, `components/editor.rs` 902) and the guard passed in the 2026-11-13 `full`. | The comment contract for this milestone is genuinely intricate — the grammar's strings versus `"//"`, the mixed-range rule, the pair-is-whole-range case, the selection republish — and the plan asked for that prose. The ratchet is the mechanism that makes the cost deliberate, and re-banking is how it stays deliberate. | None for this milestone. One tree-wide violation has since returned in `src/tui/components/dialogs/keybind.rs` (ratio 0.0723 against 0.0676), created by the corrective plan's WP D deletion at 2026-11-13 20:03 and owned by that work package's manifest bank. |
| low | **The plan's `.md` token-lookup case has no test**, and with it the block-only fallback branch (`line: None`, `block: Some`) has no assertion. `.md`, `.css` and `.html` are exactly that branch — the case where `space c` must fall back to the pair. | The plan's §10 names `.md` explicitly. The lookup is grammar-generic and the fallback is five lines of `match`, so the risk is small, but it is a required test that does not exist and the only branch of `CommentTokens::style` with no test. | Add `markdown_declares_only_a_block_pair` (asserting `line == None`, `block == Some(("<!-- ", " -->"))`, and `style(false) == Some(Block(..))`) to `editor_highlight.rs`'s tests. Cheap, and it closes both gaps at once. The plan's parenthetical "(line token only)" should be corrected to "block pair only" when the roadmap is revised. |
| low | **No dispatch-path test asserts the `space C` pair-less refusal.** `python_declares_its_own_line_token_and_no_block_pair` asserts `style(true) == None`; the notice at `commands/editor.rs:5887-5891` is implemented but unasserted end to end. | A reader on a Python buffer pressing `space C` gets the correct notice today; a future change to the notice text or the branch could regress it silently, because no test names it. | Add one dispatch test: `space C` on a `.py` buffer, assert the notice names `.py` and "no block comment pair", the text is byte-identical, and `undo_depth == 0`. |
| low | **Three common extensions have no bundled grammar at all.** The probe shows syntect 5.3.0 ships 75 syntaxes and **no TOML grammar**, and none for `.ts` or `.lock`; `.toml` therefore refuses on `Cargo.toml`. | The most likely surprise for a reader in a Rust repository: `space c` on `Cargo.toml` states "`.toml` declares no comment syntax" instead of commenting. The refusal is correct, names the extension, and is consistent with the highlighter never colouring that file — the parity invariant holds — but the limitation is not documented. | Add one sentence to `docs/tui.md`'s refusal paragraph naming the bundled-set limit and `Cargo.toml` as the example, or record it as an accepted limitation in the roadmap. Not a defect in this milestone's code. |
| low | **Discharged 2026-11-13.** `cargo fmt --check` and `scripts/verify.sh quick` were not run by this record's author. | §4's guard evidence was the six editor guards, the render baseline, the filtered library tests and the comment budget — not the routine gate. | None: the 2026-11-13 `scripts/verify.sh full` ran `quick` and `run_guards()` (which contains `cargo fmt --check --all`) on this tree, green. |
| low | **The tree is uncommitted and was being edited while this record was written.** HEAD is `7642b9e38`; 228 paths are dirty or untracked; `src/tui/editor.rs`, `src/tui/commands/editor.rs` and `src/tui/components/editor.rs` changed at 20:46–20:50 while the review ran, and two compile breaks from the M006 lane were observed and recorded in §4. | Each figure above describes the tree at the build that produced it (20:52), not a commit. Nothing here is attributable to a SHA, and a later reader cannot reproduce the numbers from HEAD. | Take these figures as the record of a working tree, and re-run the six commands on the merged head, labelling the transcript with the commit that carries M005. |
| low | **Discharged 2026-11-13.** `tui::commands::editor::tests::a_motion_records_nothing_and_each_named_jump_records_where_it_started` failed at this record's close (`left: 8, right: 4` at `commands/editor.rs:13512`). The test still exists (`commands/editor.rs:15050`) and the 2026-11-13 `scripts/verify.sh full` ran the workspace sweep at 17,377 tests with 0 failures. | It was a **jumplist** test in a file M005 also edits, so a reader running the filter saw a red line in the M005 path; its body contains no comment-toggle assertion and its failure did not involve M005's code. | None; M006's owner resolved it, and the full run is the evidence. |
| low | **Discharged 2026-11-13 — the two `tui_render -- invariants` reds belonged to the render lane's in-flight rewrite, and the lane has settled them.** At this record's close `invariants::vocabulary::the_recorded_hint_of_every_surface_is_what_it_renders` compared a 13-entry sorted `RECORDED` table (still carrying `ProjectInit`) against a 28-entry measurement, and `invariants::baseline::the_recorded_i4_baseline_is_exactly_what_the_harness_measures` recorded lowercase `enter` against the measured `Enter` (59 against 61), while the Python guard passed at 61 / 2495 / 1. | A reader who took "the render ratchet did not move" from this record could have run the family, seen two reds and assumed M005 caused them. | None. The 2026-11-13 `scripts/verify.sh full` ran the render corpus at 1,237 tests, 0 failures, and `check_tui_render_invariant_baseline.py` now reports **4 advertised-key, 0 contrast and 0 hint-truncation** entries, every one reproducing. |

No high or critical finding. No migration, protocol variant, crate or new
authority was added.

## 11. Roadmap disposition

**Milestone conditionally closed.** The capability is complete and verified: the
token source, the geometry, the inverse property, the one-transaction rule, the
selection republish, the three refusal shapes, the read-only gate, the two
leaves, the guard's new rule and record, and the documentation are all in place
and each is asserted. The conditions are:

- **Condition.** Before M005 is cited as closed for dependency purposes:
  1. `cargo fmt --check` and `scripts/verify.sh quick` are green on the merged
     tree; — **met 2026-11-13**: both ran inside `scripts/verify.sh full`, which
     passed on this tree (17,377 workspace tests, 8,441 feature-pass tests, 0
     failures; formatting and clippy clean).
  2. `python3 scripts/check_comment_budget.py` is green, or the four M005-path
     ceilings are re-banked with a recorded reason (§10); — **met 2026-11-13 by
     the second arm**: all four ceilings are banked (`editor_highlight.rs` 313,
     `editor.rs` 3693, `commands/editor.rs` 3110, `components/editor.rs` 902)
     and the guard passed inside that `full`.
  3. M006 deletes the `j` entry from `UNAVAILABLE_SPACE_LEAVES`, so the leaf
     guard's real run is green and its recorded set reads **4** (§10, medium);
     — **met**: the real run passes with 4 reasons (`F`, `b`, `D`, `'`).
  4. the `.md` / block-only-fallback test and the `space C` pair-less dispatch
     test are added (§10, low); — **not met, and still the milestone's own
     residue**: neither test exists in this tree (no `markdown_declares_*`
     test; no dispatch test asserts the pair-less notice at
     `commands/editor.rs:6543`).
  5. the failing jumplist test is resolved or explicitly attributed (§10, low);
     — **met 2026-11-13**: the test exists (`commands/editor.rs:15050`) and the
     workspace sweep ran 17,377 tests at 0 failures.
  6. the two red `tui_render -- invariants` tests are settled by the
     render-invariants lane, so a merged-tree run of that filter is green and the
     Rust family agrees with the Python guard about the recorded baseline (§10,
     low). — **met 2026-11-13**: the render corpus ran 1,237 tests at 0
     failures, and `check_tui_render_invariant_baseline.py` reports 4 / 0 / 0
     with every entry reproducing.
- **Risk.** Low on capability, low on the gate, medium on the two missing tests.
  The capability is covered by 17 `tui::editor_highlight` tests, 229
  `tui::editor::tests` tests and 284 passing `tui::commands::editor` tests, with
  the dispatch-path tests named individually in §2. The former residual — a
  guard red for another milestone's change and two render-family reds in the
  same filter — is discharged by the 2026-11-13 `full`. What remains is
  condition 4: two required test cases that still do not exist.
- **Exact future evidence.** Satisfied for conditions 1, 2, 3, 5 and 6 by the
  2026-11-13 `scripts/verify.sh full` run and the re-banked ceilings. Still owed
  for condition 4: the `.md` block-only test and the `space C` pair-less
  dispatch test, plus the commit that carries this plan (WP 0).

M006 and later milestones may proceed. They must not widen the prefix machine
for `space Alt-c` (it needs a modified-leaf table and its own guard change), and
they must keep `Ctrl-c` as the exit chord.

## 12. Registry updates

- Mark roadmap M005 **conditionally closed** and link this record.
- Record the guard figures at this tree state: key authority **2 / 90 / 8** with
  no shadowed arm (unchanged from M004 — M005 adds no chord), doc drift **52 leaf
  gestures / 98 chords**, leaf authority **6 tables / 52 glyphs / 13 bindings**,
  render baseline **61 / 2495 / 1**. The render baseline's *Rust* family is
  separately red for the render lane's own half-finished rewrite (2 of 31 tests
  in `tui_render -- invariants`); that is recorded so a reader does not attribute
  it to M005, not as a claim about the ratchet.
- Record that `UNAVAILABLE_SPACE_LEAVES` was **5** at M005's close (`F`, `b`,
  `j`, `D`, `'`) and became **4** when M006's `space j` landed. At the time of
  writing rule 6 reported the stale row and did so in the real run; it does not
  now — the 2026-11-13 run passes with 4 reasons. At HEAD the space table had
  **7** `Unavailable` rows and the guard had no record at all.
- Record the `metadata` feature as **load-bearing**, not merely enabled: reverting
  `Cargo.toml` alone breaks the build with `E0599` at
  `src/tui/editor_highlight.rs:572`. Record the measured cost with its unit —
  `syntect` rlib **+3,346,576 B (+27.0 %)** as measured by the implementing
  session, reproduced here at **+27.33 %** on a throwaway project (the absolute
  difference is the workspace's `strip = "debuginfo"` profile) — and the probe
  binary **+343,040 B (+20.7 %)**, which was reported and not reproduced.
- Record the accepted limitations the docs do not yet state: the bundled grammar
  set (**75 syntaxes**) has no TOML grammar, so `space c` on `Cargo.toml`
  refuses; and `.md`/`.css`/`.html` are the block-only branch that the missing
  `.md` test would cover.
- Do **not** mark M005 closed on the strength of this record alone: the
  conditions in §11 are what stand between conditional closure and closure.

This record does not edit `plans/registry.md`, the source plan's `Status:` line,
or the roadmap's status table. Those are owned by a separate sweep; this record's
author was scoped to `plans/closure/**` only, and every status claim above is a
recommendation for that sweep rather than a change it made. **At the time of
writing, the source plan's `Status:` line is still `ready for handoff` and the
roadmap's §12 row for M005 is still `ready for handoff`**, while the roadmap's
own header line reads "M001–M005 implemented; M006 in flight" and
`plans/registry.md` reads "M005 implemented (record pending)".

## Correction log (2026-11-13)

- **§1, the leaf-guard paragraph** — "The guard is red in this tree today …
  the record still lists `j`, so the real run reports one violation" → `j` is
  gone; the real run passes with 4 unavailable leaf reasons and 15 prefix
  bindings. Discharge: M006's deletion of the `j` row, confirmed by
  `python3 scripts/check_tui_editor_leaf_authority.py` and by the 2026-11-13
  `scripts/verify.sh full`.
- **§1, the render-ratchet paragraph** — "The **Rust** side of that lane is
  separately red in this tree … so the Python guard and the Rust family
  disagree" → the lane settled both reds; the 2026-11-13 `full` ran the render
  corpus at 1,237 tests / 0 failures and `check_tui_render_invariant_baseline.py`
  now reads 4 / 0 / 0. The 61 / 2495 / 1 figures stay as that tree's
  measurement.
- **§2, the leaf-guard row** — "Real run: one rule-6 violation, for `j` only"
  → real run passes with 4 reasons (2026-11-13).
- **§4, the leaf-guard result row** — "**fail** — 1 violation …" → marked
  superseded: the 2026-11-13 run passes with 4 reasons and 15 prefix bindings.
- **§4, the comment-budget result row** — "**fail** — 52 comment-budget
  violation(s) … None of those four ceilings was re-banked" → marked
  superseded: all four ceilings are banked in `docs/comment-budget.toml`
  (`editor_highlight.rs` 313, `editor.rs` 3693, `commands/editor.rs` 3110,
  `components/editor.rs` 902) and passed inside the 2026-11-13 `full`.
- **§4, `Not run`** — "`cargo fmt --check` … Not run" and "`quick`/`full` Not
  run" → both discharged by the 2026-11-13 `scripts/verify.sh full`
  (`full` → `quick` → `run_guards()`, whose first line is
  `cargo fmt --check --all`).
- **§10, medium (leaf guard red)** — "so the real run reports one rule-6
  violation" → the red is discharged; the real run passes with 4 reasons.
- **§10, medium (comment ratchet)** — "reports 52 violations tree-wide … None
  of the four ceilings was re-banked" → the four M005-path ceilings are banked
  and the guard passed in the 2026-11-13 `full`; one unrelated `keybind.rs`
  violation has since appeared from WP D.
- **§10, low (`fmt`/`quick` not run)** — discharged by the 2026-11-13 `full`.
- **§10, low (jumplist test failing)** — "fails (`left: 8, right: 4`)" → the
  test exists and the workspace sweep ran 17,377 tests at 0 failures.
- **§10, low (two `tui_render -- invariants` reds)** — discharged: render corpus
  1,237 / 0, Python guard 4 / 0 / 0.
- **§11, conditions 1, 2, 3, 5 and 6** — each marked met with its discharge
  named. **Condition 4 remains open**: the `.md` block-only test and the
  `space C` pair-less dispatch test still do not exist (no
  `markdown_declares_*` test in `editor_highlight.rs`; the notice at
  `commands/editor.rs:6543` is unasserted end to end).
- **§12, the registry bullet for the unavailable set** — "will otherwise
  report the stale row, and does today" → corrected: it reported it then, and
  the 2026-11-13 run passes with 4 reasons.
