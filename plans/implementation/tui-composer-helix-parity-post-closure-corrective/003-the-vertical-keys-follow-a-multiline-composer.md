# TUI Composer Helix Parity Post-Closure Corrective Milestone 003 — The Vertical Keys Follow a Multiline Composer

Status: ready

Repository baseline: `c2d781319` (`origin/main`)

Source addendum: `plans/subsystems/tui-composer-helix-parity-post-closure-corrective-addendum.md#c003--the-vertical-keys-follow-a-multiline-composer`

Parent roadmap: `plans/subsystems/tui-composer-helix-parity-roadmap.md`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/001-terminology-and-domain-model.md#prompt`
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-session-tui`

Applicable ADRs:

- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` (**accepted** 2026-10-19, amended
  2026-10-19 and 2026-10-23) — governs the composer's claim set and the `j`/`k` condition. **This
  milestone amends that record a second time**, by a dated section appended to it, following the
  precedent its own 2026-10-23 amendment set. The Decision text is not edited.
- `plans/subsystems/tui-composer-text-layout-roadmap.md` — owns the composer's word wrap, its
  visual-row geometry and the app-level page routing. Its M002 is the plan that recorded the
  `Ctrl+u`/`Ctrl+d` ownership; that milestone stays **closed**, and this milestone records the
  change to the condition it used.

Corrects: the closed milestone **M004** (composer motions), whose closure record is
`plans/closure/tui-composer-helix-parity/004-status.md` — it closed the `j`/`k` verbs against the
condition ADR-0042's 2026-10-23 amendment set — and the **conditionally closed** M002 of the
`tui-composer-text-layout` roadmap (`plans/closure/tui-composer-text-layout/002-status.md`), which
recorded the app-level `Ctrl+u`/`Ctrl+d` routing. Neither milestone is reopened; this one changes
the **condition** they recorded, and the ADR's record of it.

Primary class: capability (the vertical keys belong to a multiline composer), with two
invariants (the published keymap is the implemented keymap; one fact, one predicate) and one
piece of infrastructure (the published keymap's guard).

## 1. Objective

`j`, `k`, `Ctrl+u` and `Ctrl+d` belong to the composer **while its content occupies more than
one row**, and return to the transcript when it does not. The rule is stated once, in the
implementation and in both published tables, and it is machine-checked.

## 2. Why this milestone is ready

- **No hard dependencies.** M001–M007, the first corrective, and the composer-text-layout
  roadmap are closed. C002 must land first only because this milestone's guard reads the `v`
  wording C002 publishes.
- **The published keymap and the implementation already disagree.** `architecture/tui.md`'s
  claim table states the condition as **"composer buffer has more than one line"**
  (`architecture/tui.md:2159-2160`), while the implementation's condition label is
  **"composer content overflows the box"** (`src/tui/composer_claims.rs:263`) and `docs/tui.md`
  was rewritten to match the implementation (`docs/tui.md:379-381`, `:399`). The divergence is
  the 2026-10-23 amendment's doing, and it is exactly the class of drift this milestone's guard
  is for: the amendment changed the **implementation** and one published document, and left the
  other published table describing the old rule.
- **The user's request is the published rule, restored and made precise.** The rule the
  architecture table has always stated — more than one line of composer text — is the rule the
  user asks for. The milestone implements it in the measure that satisfies both it and the
  2026-10-23 amendment's concern (decision C-D1: the **rendered row** count).
- **The predicate's inputs already exist.** `ComposerFit` already carries the measured
  `row_count` and `visible_rows` (`src/tui/composer_claims.rs`), and the gate already reads the
  **cached** fit (`src/tui/app/mod.rs:4166-4191`), so the change is one predicate over a fact
  that is already measured.
- **The routing shape does not change.** `Ctrl+u`/`Ctrl+d` are app-level chords today, routed at
  the app layer and not through the claim table (decision C-D2). The milestone changes the
  **predicate** both call sites read, not the routing's shape, and it adds no claim-table row.

## 3. Current implementation evidence

### 3.1 The condition

| Site | Fact |
|---|---|
| `src/tui/composer_claims.rs:221-227` | `ClaimCondition::{Always, ContentOverflowsBox}` — the only conditional claim. |
| `src/tui/composer_claims.rs:255-266` | The condition's **label**, the words the help and the hint row use: `"composer content overflows the box"`. |
| `src/tui/composer_claims.rs:339-356` | `holds` reads the **cached** fit: `ContentOverflowsBox => fit.overflows()`. |
| `src/tui/composer_claims.rs` (`ComposerFit`) | The cached measurement: `row_count` (the wrapped rows the content needs) and `visible_rows` (the rows the box shows); `overflows()` is `row_count > visible_rows`. |
| `src/tui/composer_claims.rs:495-508` | `j` and `k` carry `ClaimCondition::ContentOverflowsBox`; every other claim is `Always`. |
| `src/tui/app/mod.rs:4166-4191` | The gate: the composer claims a letter only when the condition holds against the cached fit, and only in the resolved Normal mode. |
| `src/tui/components/prompt.rs` (`content_overflows`, `page_up`, `page_down`) | The widget's overflow predicate and its page motions; the page moves the window **and** the caret by a page of visual rows. |
| `src/tui/app/mod.rs:4418-4438` | `Ctrl+u`/`Ctrl+d` reach `PageUp`/`PageDown`, which route to `prompt.page_up()`/`page_down()` when `prompt.content_overflows()` and to the transcript otherwise. |

### 3.2 The published keymap

| Site | Fact |
|---|---|
| `architecture/tui.md:2152-2167` | The claim table, with a **Condition** column. `j` and `k` read **"composer buffer has more than one line"**; every other row reads "always" (`U` reads "always (both encodings)"). |
| `architecture/tui.md:2729-2733` | "**`Ctrl+u` and `Ctrl+d` page the composer when it overflows and the transcript when it does not**". |
| `docs/tui.md:376-381` | "`j` and `k` only move the caret when your prompt **does not fit its box**". |
| `docs/tui.md:383-386` | "`Ctrl+u` and `Ctrl+d` **page**. They page the **composer** when the composer has more to show than its box can hold…". |
| `docs/tui.md:396-402` | The `[NOR]`-mode keymap table: `j` / `k` — "only when the prompt does not fit its box". |
| `src/tui/composer_claims.rs` (`composer_verb_entries`) | The derived verb list the hint row and the help read; the condition's label appears in both. |
| `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md:421-483` | The 2026-10-23 amendment: the condition changed from "more than one line" to "does not fit its box", so that a wrapped single line that overflows stays reachable. |

### 3.3 The divergence, stated plainly

| Source | The condition it states |
|---|---|
| `architecture/tui.md`'s claim table | "composer buffer has more than one line" |
| `architecture/tui.md`'s page paragraph | "overflows … when it does not" |
| `docs/tui.md`'s two tables and its prose | "does not fit its box" |
| The implementation's label and predicate | "overflows the box" |
| ADR-0042's Decision table | "more than one line" |
| ADR-0042's 2026-10-23 amendment | "does not fit its box" |

**The ADR's amendment, `docs/tui.md` and the implementation say one thing; the ADR's decision
table and the architecture table say another.** No guard reads those tables, so nothing failed.

### 3.4 Why the existing verification passed

The composer-text-layout milestone verified that the implemented condition matched the amended
ADR, and `scripts/check_tui_composer_key_authority.py` verified that the claim table's
conditions were known to it. Both are true, and both are silent about **which** boundary is
right and about whether the published tables agree with the implementation. The condition was a
decision, and a decision is not a requirement — nothing tested it.

## 4. Invariants that must not regress

- **I1 — one fact, one predicate.** "Is the composer multiline?" is one derived fact with one
  predicate over the cached fit. The claim table's condition and the app-level page routing read
  **that** predicate. Neither computes a second answer to it.
- **I2 — the gate reads the cached fit.** The condition is evaluated against the fit the layout
  pass measured, never against a second measurement (`src/tui/composer_claims.rs:339-345`).
- **I3 — an unknown fit does not hold.** A claim that fired on an unmeasured composer would
  swallow a key in a state the user cannot see (`src/tui/composer_claims.rs:349-352`).
- **I4 — the claim set is unchanged.** The same thirteen letters, the same spellings, the same
  encodings. `j` and `k` keep their conditional status; the **condition** changes.
- **I5 — the routing shape is unchanged.** `Ctrl+u`/`Ctrl+d` remain app-level chords routed at
  the app layer, and they remain **not mode-gated** (`architecture/tui.md:2732`): they behave the
  same in both modes.
- **I6 — the transcript's scroll is unchanged.** What changes is which surface owns the key.
- **I7 — the composer's page moves the caret with the window.** The page moves the window **and**
  the caret by a page of visual rows (`src/tui/components/prompt.rs:750-775`), so a page in a
  composer whose content fits still moves the caret — as long as the caret is not already on the
  last row. This is what makes the new condition usable: under the old condition a fitting
  composer never paged; under the new one it does, and the caret moves. The qualifier matters —
  `move_visual_row` clamps the target to the last row (`:738`) and returns early when the target
  row equals the current row (`:740-744`), so a **one-row** composer does not move its caret on a
  page at all, and neither does a caret already on the last row. That is why the new condition is
  *multiline* and not merely *fits*: the case the milestone enables is exactly the case where the
  caret can move.
- **I8 — the published keymap states one rule.** After the milestone, the architecture table,
  the architecture prose, `docs/tui.md`'s two tables and its prose, and the implementation's
  label all say the same thing, and the ADR's amendment records the change.

## 5. Scope

**In scope.**

- The claim condition and its label.
- The app-level `Ctrl+u`/`Ctrl+d` routing's predicate.
- The dated ADR-0042 amendment.
- The published tables and prose in `architecture/tui.md` and `docs/tui.md`.
- The new guard `scripts/check_tui_composer_keymap_drift.py`, its `--self-test`, and its
  registration in `scripts/verify.sh`'s `run_guards()` **and** in `.github/workflows/ci.yml`.
- The verified-counts re-measurement and the reconciliation in `architecture/count_audit_history.md`.

**Out of scope.**

- The composer's motion set, its selection semantics (C002) and its caret (C001).
- The transcript's own scroll behaviour.
- The key authority guard's **rules**. Its `KNOWN_CONDITIONS` set learns the new condition's
  name; the rules themselves are unchanged.
- The keymaps, the app's binding tables and the claim set.
- The editor.

## 6. Required production changes

### 6.1 The predicate

One predicate over the cached fit:

> **The composer is multiline when its content occupies more than one rendered row** —
> `row_count > 1`.

The rendered row count is the measure (decision C-D1), and the reason is that it satisfies both
the user's rule and the 2026-10-23 amendment's concern:

- A composer holding a **wrapped single line** that overflows its box is multiline by this
  measure, so its content stays reachable — the concern the 2026-10-23 amendment recorded.
- A composer holding **three lines that fit** is multiline by this measure, so it keeps the
  vertical keys — the user's rule.

`ComposerFit` grows the predicate; the claim condition and the app-level routing both call it.
**`PromptWidget::content_overflows()` is replaced, not left behind.** It is the method the
app-level routing calls (`src/tui/app/mod.rs:4421`, `:4433`), and it is the composer's only
overflow predicate (`src/tui/components/prompt.rs:483-485`). Leaving it in place would keep
`fit.overflows()` alive behind a second name, and the gate and the page branch would then answer
two different questions about one composer. The milestone therefore **replaces** it with the
multiline predicate (the implementer chooses the name) and re-points **every** call site:

| Call site | Kind | What it must become |
|---|---|---|
| `src/tui/app/mod.rs:4421`, `:4433` | production — the page branch | The multiline predicate |
| `src/tui/composer_claims.rs:353` | production — the claim gate | The multiline predicate, through the claim's own condition |
| `src/tui/components/prompt.rs:1279`, `:1299`, `:1314`, `:1491` | the widget's own tests | The multiline predicate, or a test of the predicate itself |
| `tests/tui_render/tui_composer_claims.inc:635`, `:658`; `tests/tui_render/tui_composer_motions.inc:199`, `:268`, `:315`, `:360`, `:393` | the render family | The multiline predicate's behaviour |

`ComposerFit::overflows()` is then removed if nothing else calls it, rather than left as a second
answer to a question nobody asks any more, and the key authority guard's `KNOWN_CONDITIONS` set
(`scripts/check_tui_composer_key_authority.py:170`) learns the new condition's name in place of
the old one. The closure record names the choice.

### 6.2 The claim condition

`ClaimCondition::ContentOverflowsBox` becomes the multiline condition, with a label that says
what it is in the words the help and the hint row use. The label is **published**, so it is
stated once and read by the guard (§6.5).

The two conditional claims (`j`, `k`) carry it; the other eleven stay `Always`.

### 6.3 The page routing

`Ctrl+u`/`Ctrl+d` route to the composer when the composer is multiline, and to the transcript
otherwise. The routing's shape — the same branch shape the workspace dashboard uses
(`architecture/tui.md:2729-2733`) — is unchanged; only the predicate it asks changes.

### 6.4 The ADR-0042 amendment

A dated section **appended** to the accepted record, in the shape the 2026-10-23 amendment
established: what changes, why, what is preserved, and why it is an amendment rather than a new
decision. Its content:

- Decision part 4's `j`/`k` rows take the **multiline** condition, replacing the 2026-10-23
  amendment's "does not fit its box".
- The 2026-10-23 amendment's **reason is preserved**: a wrapped single line that overflows stays
  reachable, because an overflowing composer is necessarily multiline.
- What changes is the boundary in the other direction: a composer that is multiline and
  **fits** now keeps the vertical keys.
- The trade-off is recorded: while the composer is multiline, the transcript's `j`/`k` are
  unavailable, and the recovery is automatic — the composer returns to one row and the keys
  return to the transcript.
- The record notes, for the avoidance of doubt, that the app-level `Ctrl+u`/`Ctrl+d` routing
  follows the same predicate. It adds **no** claim-table row.

### 6.5 The guard

`scripts/check_tui_composer_keymap_drift.py` is **new**. It reads:

| Input | Where |
|---|---|
| The claim table | `src/tui/composer_claims.rs` (`COMPOSER_CLAIMS`) |
| The condition's label | `src/tui/composer_claims.rs` (`ClaimCondition::label`) |
| The published claim table | `architecture/tui.md`'s composer claim table |
| The published keymap tables and prose | `docs/tui.md`'s composer sections |

And it fails when:

| Rule | The violation |
|---|---|
| **D1** | A letter in the claim table is missing from the published table, or a published letter is not in the claim table. |
| **D2** | The set of **conditional** verbs in the published table differs from the claim table's conditional claims. |
| **D3** | The published condition's wording differs from the implementation's condition label. |
| **D4** | The published prose about the condition (`docs/tui.md`'s paragraphs, `architecture/tui.md`'s page paragraph) states a different rule from the tables. |
| **D5** | A table, a row or an input the rule needs **cannot be found**. A rule that finds nothing is a violation, not a clean tree — the precedent is `scripts/check_tui_files_keymap_drift.py`. |

**The guard must be armed, not merely present.** The milestone records that the guard **fails**
against the pre-correction tree — the divergence in §3.3 is its first finding — and that
`--self-test` proves each rule rejects its violation class. The guard is registered in
`scripts/verify.sh`'s `run_guards()` **and** in `.github/workflows/ci.yml`.

### 6.6 The counts

The new guard changes two rows of `architecture/overview.md`'s verified-counts table. The counts
are re-measured with `python3 scripts/measure_overview_counts.py` and the reconciliation is
recorded in `architecture/count_audit_history.md`, in the format that file already uses.

## 7. Ordered work packages

- **WP1 — the predicate.** Add the multiline predicate over the cached fit; decide and record
  what happens to `overflows()`.
- **WP2 — the claim condition.** Replace the condition, its label and its `holds` arm.
- **WP3 — the page routing.** Point the app-level `Ctrl+u`/`Ctrl+d` branch at the same
  predicate.
- **WP4 — the ADR amendment.** Append the dated amendment to ADR-0042.
- **WP5 — the published tables.** `architecture/tui.md`'s claim table and its page paragraph;
  `docs/tui.md`'s two tables and its two paragraphs. All four state the new rule in the same
  words as the implementation's label.
- **WP6 — the guard.** Write the guard and its `--self-test`; register it in `scripts/verify.sh`
  and in `.github/workflows/ci.yml`; record that it fails against the pre-correction tree.
- **WP7 — the counts.** Re-measure and reconcile (§6.6).
- **WP8 — the tests.** §10.
- **WP9 — the verification.** §11, in order, with the results recorded in the closure record.

## 8. Failure, cancellation, restart, contention semantics

None. The predicate reads a cached measurement and returns a bool. No state, no task, no timer.

## 9. Compatibility and migration

- **Storage:** none.
- **Protocol:** none.
- **Migration:** none.
- **User-visible change:** while the composer holds more than one row of text, `j`, `k`,
  `Ctrl+u` and `Ctrl+d` move inside the composer. When it returns to one row, they scroll the
  conversation again. The recovery needs no key.
- **The claim set, the keymaps and the binding tables are unchanged.**

## 10. Required tests

### 10.1 Unit tests — `src/tui/composer_claims.rs`, `src/tui/components/prompt.rs`

| Test | Asserts |
|---|---|
| `a_composer_with_one_row_is_not_multiline` | A one-row fit does not hold the multiline condition. |
| `a_composer_with_several_rows_is_multiline` | A fit with more rows than the box shows holds it — **and** a fit with several rows that still fit holds it. |
| `the_condition_label_names_the_multiline_rule` | The label's words are the words the published tables use (the guard reads both, so the label is the shared vocabulary). |

### 10.2 Render and dispatch tests

| Test | Asserts |
|---|---|
| `a_multiline_composer_takes_the_vertical_keys` — `tests/tui_render/tui_composer_claims.inc` | With a composer holding more than one row **that fits its box**, `j` moves the composer's caret and the transcript does not scroll. |
| `a_single_row_composer_leaves_the_vertical_keys_to_the_transcript` — `tests/tui_render/tui_composer_claims.inc` | With a one-row composer, `j` scrolls the transcript, as today. |
| `a_multiline_composer_takes_the_page_keys` — `tests/tui_render/modifier_chords.inc` | With a multiline composer that fits, `Ctrl+d` pages the composer — and the caret moves, because the page moves the caret as well as the window (invariant I7). |
| `a_single_row_composer_leaves_the_page_keys_to_the_transcript` — `tests/tui_render/modifier_chords.inc` | With a one-row composer, `Ctrl+d` pages the transcript, as today. |
| `the_vertical_motions_still_work_when_the_composer_overflows` — `tests/tui_render/tui_composer_motions.inc` | The overflowing case — the one the 2026-10-23 amendment was recorded for — still moves the caret. |

### 10.3 The existing members that name the condition

Several existing members **name** the overflow condition in their own names and assertions, and they
are re-pointed, not deleted: `k_is_claimed_when_the_composers_content_overflows_its_box`
(`tests/tui_render/tui_composer_claims.inc:651`), `a_composer_whose_content_overflows_its_box_overflows`
(`src/tui/components/prompt.rs:1306`), `ctrl_d_pages_the_composer_when_its_content_overflows`
(`tests/tui_render/tui_composer_motions.inc:355`) and the assertions listed in §6.1. A test whose
name says "overflows" while its assertion says "multiline" is a second vocabulary, and the guard
reads the names it can see.

### 10.4 The guard

| Test | Asserts |
|---|---|
| `scripts/check_tui_composer_keymap_drift.py --self-test` | Every rule's violation class is detected and rejected, and a clean tree passes. The milestone records the run against the **pre-correction** tree, where the guard must fail. |

## 11. Required verification commands

```bash
cargo test -p codegg --lib -- composer_claims
cargo test --test tui_render -- tui_composer_claims
cargo test --test tui_render -- tui_composer_motions
cargo test --test tui_render -- modifier_chords
python3 scripts/check_tui_composer_key_authority.py
python3 scripts/check_tui_composer_key_authority.py --self-test
python3 scripts/check_tui_composer_keymap_drift.py
python3 scripts/check_tui_composer_keymap_drift.py --self-test
python3 scripts/measure_overview_counts.py
python3 scripts/check_comment_budget.py
CARGO_BUILD_JOBS=8 cargo clippy --workspace --all-targets --locked -- -D warnings
bash scripts/verify.sh quick
```

`scripts/verify.sh quick` is the canonical gate, and it now runs the new guard.

## 12. Documentation updates

| Document | Change |
|---|---|
| `architecture/tui.md:2152-2167` | The claim table's **Condition** column for `j` and `k`: the multiline rule, in the implementation's words. |
| `architecture/tui.md:2729-2733` | The page paragraph: the composer pages when it is multiline, the transcript when it is not. |
| `docs/tui.md:376-386` | The prose under the word motions: the same rule, in the same words. |
| `docs/tui.md:396-402` | The `[NOR]`-mode keymap table's `j` / `k` row. |
| `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` | The dated amendment (§6.4), **appended**; the Decision text is not edited. |
| `src/tui/composer_claims.rs` | The condition's doc comment: what "multiline" means and why the rendered row count is the measure. |
| `architecture/overview.md` | The verified-counts table's guard rows, after the re-measurement. |
| `architecture/count_audit_history.md` | The reconciliation entry. |

The parent roadmap, the composer-text-layout roadmap and every closed closure record are **not**
rewritten: they are the history of what was decided then, and this milestone's closure record
states the correction.

## 13. Acceptance criteria

1. A composer holding more than one row of text — **whether or not it fits its box** — takes
   `j`, `k`, `Ctrl+u` and `Ctrl+d`. Tests assert both the multiline-fitting and the
   multiline-overflowing cases.
2. A one-row composer leaves all four keys to the transcript. Tests assert it.
3. The page in a composer whose content fits moves the **caret**, not only the window.
4. One predicate answers "is the composer multiline?", and the claim table and the page routing
   both read it. The closure record names the predicate and its two callers.
5. The implementation's condition label, `architecture/tui.md`'s claim table, `architecture/tui.md`'s
   page paragraph, `docs/tui.md`'s two tables and its two prose paragraphs state **one** rule in
   **one** wording. The new guard proves it.
6. ADR-0042 carries a dated amendment recording the change, its reason, what is preserved and
   the trade-off; its Decision text is unchanged.
7. The new guard is registered in `scripts/verify.sh` **and** in `.github/workflows/ci.yml`, its
   `--self-test` passes, and the milestone records that the guard fails against the
   pre-correction tree.
8. `architecture/overview.md`'s verified-counts table is re-measured and reconciled, and
   `architecture/count_audit_history.md` records the reconciliation.
9. `scripts/verify.sh quick` passes, and clippy is clean with `-D warnings`.

## 14. Stop conditions

- The multiline predicate cannot be derived from the cached fit without a second measurement.
  A second computation of the same fact is a second answer to it; the milestone stops with the
  evidence.
- The guard cannot read the published tables mechanically — that is, the tables cannot be parsed
  into a comparable form without rewriting them into a shape no document should take. The
  milestone stops and the guard's design is escalated; a guard that only works on a rewritten
  document guards nothing.
- The change requires a claim-table row for the `Ctrl+u`/`Ctrl+d` chords, which decision C-D2
  says it does not. The milestone stops rather than extending the claim table's key space.
- The amendment to ADR-0042 cannot be written as an appended, dated section. The milestone stops;
  the ADR's record is not rewritten to fit.
- Any existing `tui_render` member fails for a reason other than the condition's wording. That
  is a regression, and the milestone stops.

## 15. Closure evidence required

- The closure record `plans/closure/tui-composer-helix-parity-post-closure-corrective/003-status.md`,
  with: the predicate and its two callers; the behaviour before and after for each of the four
  keys in each of the three composer states (one row, several rows that fit, several rows that
  overflow); the ADR amendment's text; the guard's rules and the run that proves it is armed;
  the counts' re-measurement and the reconciliation; and the explicit statement that the claim
  set, the keymaps and the transcript's scroll are unchanged.
- The new guard's name, its `--self-test` output and its registration in both gates.
- The exact wording used in the implementation's label and in both published tables, recorded as
  **one** string.

## 16. Handoff notes

- The guard is the piece that keeps this milestone's fact true. It must be armed with the same
  wording the implementation uses — that string is recorded in the closure record and is the
  shared vocabulary between the code, the two documents and the guard.
- C002 publishes the `v` wording that the guard's rule D1 reads through the claim table's row
  descriptions. If the guard compares descriptions as well as letters, C002 must land first.
- The composer-text-layout roadmap's M002 recorded the page routing's overflow condition. That
  milestone is closed and its record is not rewritten; this milestone's closure record states
  the change and points at the ADR amendment.
