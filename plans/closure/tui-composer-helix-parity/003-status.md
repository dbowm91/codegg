# TUI Composer Helix Parity M003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-composer-helix-parity/003-composer-text-model.md`

Source subsystem roadmap:

- `plans/subsystems/tui-composer-helix-parity-roadmap.md` (§7, M003)

Repository baseline reviewed: `2b44be918` (branch `feat/auto-20261019-8cc543fb`)

Implementation commits or pull requests:

- *uncommitted at the time of writing* — the working tree carries the shared geometry module, the
  composer's model, the editor's migration, the generic shared selection rules, this record, and
  the plan. No hosted CI run is claimed.

Applicable ADRs:

- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` — **accepted** 2026-10-19. This
  milestone does not change its decision: it adds no verb and claims no key.

## 1. Executive finding

The composer has a **text model**, and the word geometry it uses is the same geometry the editor
uses.

At the baseline the composer was a `String` and a `usize` inside its widget, with no selection, no
history, and no word motions — and the editor owned a private copy of the word rule, so "where a
word ends" was a question with two answers waiting to happen. M003 closes both gaps:

- `src/tui/composer_text.rs` gives the composer a point type that satisfies the shared selection
  contract, a selection state held by the shared state machine, a bounded undo/redo history, and
  the buffer arithmetic that used to live in the widget. `PromptWidget` keeps its public API and
  renders the model.
- `src/tui/text_geometry.rs` owns the word and line geometry over a `TextSource`, with the editor
  migrated onto it and the composer adapting its own `String`. The editor's private `is_word_byte`
  is deleted; there is one word policy.
- `src/tui/helix_selection.rs`'s rules are now generic over the point type and run against the
  composer's point **and** the terminal's. Before this milestone the rules were checked against
  one point type, so a second surface could claim to use the shared machine without being checked
  against it.

No key handling and no rendering change: the claim table is untouched, and the composer's existing
suites pass unchanged.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| **AC-M1 — the composer has a point type that satisfies the shared selection trait, and the shared selection rules pass against it** | `src/tui/composer_text.rs`: `ComposerPoint` implements `SelectionPoint` with `Line = usize`; `src/tui/helix_selection.rs`: the rules are generic and run against `ComposerSurface` | pass | 20 rules × 2 surfaces. The composer surface's points are byte offsets into a fixed-width test buffer, so a rule about "column 999" means the same thing on both surfaces. |
| **AC-M2 — the composer's selection is held by the shared selection state machine** | `ComposerText` holds a `SelectionState<ComposerPoint>`; the caret **is** the machine's cursor, not a copy of it | pass | A surface that kept its own cursor next to the machine's would have two answers to "where is the caret", and the growth rule would be applied to one of them. |
| **AC-M3 — the composer's word boundaries are identical to the editor's, and the shared policy is the only word rule** | `src/tui/text_geometry.rs::tests::a_string_and_a_snapshot_agree_on_word_boundaries` — the same text as a `&str` and as a `DocumentSnapshot`, compared at **every** offset | pass | The editor's private `is_word_byte` and `byte_at` are deleted; `text_geometry::is_word_byte` is the policy. The interactive terminal panel's whitespace-delimited rule is a *different* rule on purpose and now lives beside it as `is_whitespace_token_char`, so the tree has one module that owns word classification. |
| **AC-M4 — the undo history is bounded in depth and in bytes, and an eviction does not corrupt it** | `src/tui/composer_text.rs::tests::the_history_is_bounded_in_depth`, `::the_history_is_bounded_in_bytes`, `::an_eviction_does_not_strand_a_redo` | pass | 200 edits and 64 KiB by default. Eviction removes the oldest entry, which is safe only because a new edit clears the redo stack first — that invariant is stated in the code and asserted by the test. |
| **AC-M5 — undo and redo are inverse over the composer's buffer, including a selection deletion** | `::undo_and_redo_are_inverse`, `::undo_walks_back_through_several_edits`, `::a_selection_deletion_is_undoable` | pass | The selection deletion follows the editor's convention: a character selection is **inclusive of the head character**, a line selection spans whole lines including their terminators. |
| **AC-M6 — submitting or replacing the composer clears the undo history** | `::replacing_the_text_drops_the_history`, `::clearing_the_text_drops_the_history`; `ComposerText::clear` and `set_text` both clear the history | pass | The submit path is `src/tui/app/prompt_turn.rs`, which calls `prompt.clear()`. A restored prompt (`app/state/prompt.rs`) and a projection restore (`app/mod.rs`) call `set_text`, which also clears it. |
| **AC-M7 — the composer's rendering and behaviour are unchanged** | `tests/tui.rs` (167), `tests/tui_composer_claims.rs` (29), `tests/tui_composer_cursor.rs` (24) — all pass unchanged; `PromptWidget`'s public API is identical | pass | 183 call sites across the app use the widget's accessors, and none of them moved. |
| **AC-M8 — the editor's public geometry API is intact and its results are unchanged** | `cargo test --lib tui::editor`; the editor's public functions keep their signatures and delegate to the shared module | pass | The migration is a move, not a rewrite: the `DocumentSnapshot` adapter calls the same snapshot methods the editor called, and `line_of` uses the snapshot's indexed `byte_to_position`. |
| **AC-M9 — no key handling and no rendering change** | `src/tui/composer_claims.rs` is unchanged in content (13 rows, one `Live` verb); the claim guard passes | pass | No verb was added, no key was claimed, and the composer's drawing is untouched. |
| **AC-M10 — the workspace gates are green** | §4 | pass | `verify.sh quick` green; workspace clippy green on 1.89.0 and on 1.99.0 (the CI toolchain); `cargo fmt --all --check` clean. |

## 3. Production implementation evidence

### 3.1 The shared geometry — `src/tui/text_geometry.rs` (new)

A `TextSource` trait is the minimum a buffer has to answer for the geometry to work: its length,
its bytes, its character boundaries, its lines, and where a line starts and ends. Two adapters
implement it: `DocumentSnapshot` (the editor's daemon-authorized replica) and `str` (the
composer's buffer). The motions — `word_forward`, `word_backward`, `word_end`, `move_vertical`,
`cursor_line`, `line_start`, `line_content_end`, `buffer_content_end`, `clamp_offset`,
`next_char_boundary`, `prev_char_boundary` — are written once against the trait.

The word policy is `is_word_byte`: ASCII alphanumerics, `_`, and every byte `>= 0x80`. The last
part is what lets a scan classify a multi-byte character without decoding it — but the scan must
read that character's **first** byte, not one byte at `offset`. A one-byte read needs a character
boundary at *both* ends, so it is `Err` for the first byte of a multi-byte character as much as
for its interior. (This paragraph asserted the opposite when the milestone closed; §6.1 records
the correction.)

### 3.2 The editor's migration — `src/tui/editor.rs`

The editor's private `is_word_byte` and `byte_at` are deleted, and its public geometry functions
become thin delegations. The public API is unchanged, so no caller moved: `clamp_offset`,
`next_char_boundary`, `prev_char_boundary`, `cursor_line` and `line_start` are used outside the
editor module and keep their names and their behaviour.

### 3.3 The composer's model — `src/tui/composer_text.rs` (new)

`ComposerText` owns the buffer, the caret, the selection state and the history. `ComposerPoint`
is a byte offset paired with its line — the same shape as the editor's `EditorPoint`, and for the
same reason: the shared selection contract separates the line from the point so whole-line
selection can answer "which lines does this span" without knowing how wide a line is. The
composer is a **peer** of the editor, not a subordinate of it, so it does not borrow the editor's
type.

The undo history is two stacks with a depth bound and a byte bound. An `Edit` records the range
replaced, the text that was there, the text that replaced it, and the caret on both sides, which
is what makes undo and redo exact inverses.

### 3.4 The widget — `src/tui/components/prompt.rs`

`PromptWidget`'s `text` and `cursor` fields are replaced by the model. Every public method is
preserved, including `set_text`'s "caret at the end of the new text" and `clear`'s "the buffer is
empty". The rendering reads the model's text and caret; it draws exactly what it drew.

### 3.5 The shared rules — `src/tui/helix_selection.rs`

The rules move into a `rules` module, generic over a surface that says how to build a point at a
line and column and how to move a point down some rows. A macro then runs every rule against both
surfaces, so each rule is still its own test per surface and a failure still names the rule.

## 4. Verification executed

All commands were run locally in the worktree. **No hosted CI run is claimed for this
milestone.**

| Command | Result |
|---|---|
| `cargo fmt --all --check` | clean |
| `cargo test --lib tui::` | **2153 passed**, 0 failed (4555 filtered out) |
| `cargo test --test tui_composer_claims --test tui_composer_cursor --test tui` | see §4.1 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` (1.89.0) | pass |
| `cargo +stable clippy --workspace --all-targets --locked -- -D warnings` (1.99.0, the CI toolchain) | pass |
| `scripts/verify.sh quick` | pass |

### 4.1 The focused suites

| Suite | Result |
|---|---|
| `tests/tui.rs` | 167 passed, 0 failed |
| `tests/tui_composer_claims.rs` | 29 passed, 0 failed |
| `tests/tui_composer_cursor.rs` | 24 passed, 0 failed |

**Not run, and why.** `scripts/verify.sh full` is not green on this machine for a reason that
predates this milestone and is documented in M002's closure record: its nextest sweeps fail on 59
tests that register a workspace at `/tmp`, which macOS makes a symlink. The failure set is
identical at the baseline commit `61cbe9222`, so it is the environment, not the tree. Hosted CI
is the environment where that suite is expected to pass.

## 5. Invariant review

| Invariant | Result | Note |
|---|---|---|
| I1 — one owner per chord per surface | pass | The claim table is unchanged: 13 rows, one `Live` verb. The guard passes. |
| I2 — a claimed key never falls through | pass | Unchanged; no key handling was added. |
| I3 — the composer is a frontend-local string | pass | The model holds no daemon-authorized state, no revision and no document. It is delivered only on submit. |
| I5 — the resolved mode is the gate | pass | Unchanged; the model has no opinion about modes. |
| I7 — one redraw cadence | pass | Nothing here touches rendering or timing. The composer's drawing is unchanged. |
| The editor's behaviour is unchanged | pass | The migration moves the implementation and preserves the public API and the results. The editor's `EditorPoint` and its own selection handling are untouched. |
| The terminal panel is unchanged | pass | Its word rule is a different rule on purpose; migrating it would change what it selects. |
| The command palette's filter charset is unchanged | pass | A filter rule, not a word rule. |

## 6. Failure and recovery review

The model is a frontend-local value with no I/O, no background task and no durable state, so
there is nothing to fail, cancel or contend over. The editor migration changes no timing: the
shared `DocumentSnapshot` adapter calls the same snapshot methods, and `line_of` uses the
snapshot's indexed `byte_to_position` rather than a scan.

The one subtlety worth recording: the editor's word scan reads one byte per step through
`read_bytes`, and the shared module preserves that exactly for a `DocumentSnapshot` (its
`byte_at` default reads a one-byte range). The composer, whose buffer is a `String`, gets an
allocation-free `byte_at` instead. The migration is therefore a move rather than a rewrite.

### 6.1 A correction this milestone got wrong

**Corrected 2026-10-20, during the reconciliation with `main`.** The subtlety recorded above was
wrong, and it was wrong in the direction that mattered. The two adapters therefore classified the
*same* offset differently — the exact drift the shared module was created to remove. A
`DocumentSnapshot` one-byte read is `Err` for the **first** byte of a multi-byte character as much
as for its interior, so the editor's scan walked past every non-ASCII character, while the
composer's raw-byte read classified the same character as word content.

The TUI editor Helix-parity workstream found the same defect independently in the editor and
fixed it there — see `plans/closure/tui-editor-helix-parity/005-status.md` §5, "A pre-existing
defect found and fixed: `w`/`b`/`e` skipped all non-ASCII", and §6, "An unbounded loop, found by
sampling a stack". When that workstream was reconciled with `main`, the fix was carried **into the
shared module** rather than left on one side of it:

| | before | after |
|---|---|---|
| `DocumentSnapshot::byte_at` | one-byte range — `Err` at a multi-byte character's first byte | reads the whole character, returns its **first** byte (the editor's `leading_byte`) |
| `str::byte_at` | the raw byte, so the interior of a character was classified as word content | `None` unless the offset is on a character boundary |
| `word_end` | probed `offset + 1` | steps a whole character at a time |

Both adapters now classify identically, and `text_geometry`'s own tests assert it **on non-ASCII
text** — a case no test in this milestone covered, which is why the defect survived here. The
milestone's AC-M3 evidence is extended by
`a_string_and_a_snapshot_agree_on_word_boundaries_in_non_ascii_text` and
`a_multi_byte_character_is_a_word_and_not_a_separator`; the first of them walks every offset of
`日本語 text café naïve` and asserts that the `&str` and the `DocumentSnapshot` agree.

The other half of the editor's fix — `move_vertical`'s `desired_column`, the sticky column a
vertical run aims for — was carried into the shared module the same way, as an
`Option<usize>` parameter. The composer passes `None` (it has no sticky column); the editor
passes the column its own presentation is holding. The two surfaces therefore share the
line-motion rule rather than keeping one copy each, and the editor's feature is not lost.

**What this does not change.** The milestone's decision, its acceptance criteria, and its
conclusion all stand: there is one word rule, the editor's private copy is deleted, and the two
surfaces answer "where is a word" with one answer. What was false was the milestone's *account*
of how the rule reads a byte — and it is corrected here rather than left for the next reader to
find.

## 7. Migration and compatibility review

- **Migration.** None. Nothing is stored, and no schema or protocol changes.
- **Compatibility.** The composer renders and behaves exactly as before, and the editor's public
  geometry functions keep their signatures and their results. The widget's public API is intact,
  so the 183 app-side call sites did not move.
- **The undo history is new state.** It is bounded in both depth and bytes, it is cleared when
  the composer is submitted or replaced, and it holds no text the user cannot already see.

## 8. Security review

None. The composer remains a frontend-local string delivered to the daemon only on submit. The
model adds no authority, no path handling, no network surface and no durable record, and it
cannot change what the daemon accepts because it does not touch the submit path.

## 9. Documentation and operations

| Document | Change |
|---|---|
| `src/tui/text_geometry.rs` | New module, with the doc that states what the shared geometry is and why the two rules (word and token) are both there. |
| `src/tui/composer_text.rs` | New module, with the doc that states what the model is and — more importantly — what it is not. |
| `architecture/tui.md` | A new subsection: the composer's model, the shared geometry, the one word policy, the bounded history, and the generic shared rules. |
| `plans/subsystems/tui-composer-helix-parity-roadmap.md` | M003's status; §4's baseline paragraph, with a note that it describes the baseline; §12's status table. |
| `plans/implementation/tui-composer-helix-parity/003-composer-text-model.md` | New. |
| `plans/registry.md` | M003's row, the parent row's status, the eligible-for-planning row moved from M003 to M004, and the dependency audit. |

**No ADR was written or changed.** ADR-0042's decision is untouched: M003 adds no verb and claims
no key.

## 10. Unresolved findings

| Severity | Finding | Owner |
|---|---|---|
| low | The composer's word motions are computed by the model but **not bound to any key** — `w`, `b` and `e` remain claimed and reserved in the claim table, so they are consumed as no-ops. This is the plan's design, not a defect: the table is established before the verbs, so a claimed letter cannot leak to an app-level action while its verb is pending. | M004: the motions, and the keys that reach them. |
| low | Undo is per-edit, so undoing a typed word takes one keystroke at a time. Grouping consecutive typing into one step is a UX question with a UX answer. | M006: the undo verbs. |
| low | The interactive terminal panel's word rule is a different rule from the editor's and the composer's, and is not migrated. It is in the shared module and named for what it is, so the divergence is visible rather than hidden, but the two surfaces still answer "where is a word" differently. | Deliberate. Migrating it would change what the panel selects, which is a product decision this milestone did not take. |

No high or medium finding remains.

## 11. Roadmap disposition

- M003 is **closed**. M004 (composer motions) becomes **eligible for fresh planning**: its hard
  dependencies, M002 and M003, are both closed.
- M005–M007 are unchanged: M005 waits on M004 and M003, M006 on M005 and M003, M007 on M004, M005
  and M006.
- The parent roadmap's open decision **D4** (the undo scope: prompt text only, or the wider
  composer state) stays open and is owned by M006, which owns the undo verbs. M003 built the
  history over the prompt text, which is the recommended shape, but the decision is M006's to
  record.
- The parent roadmap's D3 remains closed (settled in M001, corrected by C001).

## 12. Registry updates

- Added M003's row to **Dependency-ready implementation plans** with this closure record.
- The parent roadmap's row now reads "M001 closed; M002 closed; M003 closed; M004 eligible for
  fresh planning".
- The **Milestones eligible for fresh planning** row moved from M003 to **M004**, with both hard
  dependencies discharged.
- **Dependency audit.** Closing M003 unblocked **M004**: the roadmap's dependency graph lists M003
  as a hard dependency of M004 ("Motions need the geometry and the caret model") and of M005
  ("Selection verbs need the selection and undo model"). M004's other hard dependency, M002, was
  already closed, so M004 is now dependency-ready and is registered as eligible for fresh
  planning. M005 still waits on M004.
