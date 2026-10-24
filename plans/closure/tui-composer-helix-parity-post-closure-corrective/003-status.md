# TUI Composer Helix Parity (post-closure corrective) Milestone 003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-composer-helix-parity-post-closure-corrective/003-the-vertical-keys-follow-a-multiline-composer.md`

Source subsystem roadmap:

- `plans/subsystems/tui-composer-helix-parity-post-closure-corrective-addendum.md` (milestone C003)

Repository baseline reviewed: `c7e5611ca` (the tree's `HEAD` when this milestone
started; C001 and C002 were already landed in the working tree and are not
re-derived here)

Implementation commits or pull requests:

- none — the change is left uncommitted in the working tree, as the milestone's
  instructions require

## 1. Executive finding

The milestone's capability boundary is **complete**. The composer's vertical
keys now follow a **multiline** composer, and one predicate answers that
question for the whole product.

The predicate is `ComposerFit::is_multiline()` — the composer's content spans
more than one **rendered row**. It is derived from the fit the layout pass
already caches, so it is not a second measurement: the claim gate and the page
routing read the same cached `row_count`. Before the milestone the question
was answered by "does the content overflow the box" (`row_count >
visible_rows`), which made a composer holding three rows that **fit** its box
lose its vertical keys. The new measure moves that boundary in the other
direction and leaves the 2026-10-23 amendment's reason intact: an overflowing
composer is necessarily multiline, so a wrapped single line that overflows
stays reachable.

The published rule is now **one string**, shared by the implementation, the two
architecture documents and the two user-facing documents, and
`scripts/check_tui_composer_keymap_drift.py` proves they cannot drift apart.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| §6.1 — the predicate is the rendered row count | `src/tui/composer_claims.rs:209` — `ComposerFit::is_multiline()` returns `row_count > 1` on `Measured` and `false` on `Unknown` | pass | One predicate, derived from the cached fit |
| §6.1 — the claim condition and the page routing both read it | `src/tui/composer_claims.rs:360` (`ClaimCondition::holds`) and `src/tui/app/mod.rs:4428,4441` (the `PageUp`/`PageDown` arms) | pass | The widget's `PromptWidget::is_multiline()` is a thin delegate to the fit's predicate, not a second answer |
| §6.1 — `PromptWidget::content_overflows()` is replaced, not left behind | `src/tui/components/prompt.rs:757` — `is_multiline()`; a search for `content_overflows` and `.overflows()` across `src/`, `tests/` and `crates/` returns **no** call site | pass | `ComposerFit::overflows()` was **removed**; nothing else called it |
| §6.2 — the condition carries the new rule, the other claims stay `Always` | `src/tui/composer_claims.rs:233` — `ClaimCondition::ContentSpansMultipleRows`; only `j` and `k` carry it (`src/tui/composer_claims.rs:508,515`) | pass | 12 claims stay `Always` |
| §6.2 — the label is published verbatim | `src/tui/composer_claims.rs:270` — `Some("composer content spans more than one row")` | pass | The guard reads the label out of `ClaimCondition::label` |
| §6.3 — `Ctrl+u`/`Ctrl+d` route to the composer when it is multiline | `src/tui/app/mod.rs:4428,4441` | pass | The branch's *shape* is unchanged; only the predicate it asks changed |
| §6.3 — the page still moves the caret with the window | `tests/tui_render/tui_composer_motions.inc` — `a_multiline_composer_takes_the_page_keys` | pass | Invariant I7 holds |
| §6.4 — the ADR carries a dated amendment, its Decision text unchanged | `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` — `## Amendment: the vertical keys follow a multiline composer` (**Amended 2026-10-24**) | pass | The Decision text is untouched; the amendment is appended before `## Supersession` |
| §6.5 — the guard is registered in both gates | `scripts/verify.sh` (`run_guards()`, two blocks) and `.github/workflows/ci.yml` (two `run:` lines) | pass | Follows the `check_tui_files_keymap_drift.py` precedent |
| §6.5 — the guard fails against the pre-correction tree | The armed run below | pass | 9 violations, exit 1, recorded verbatim |
| §6.6 — the counts are re-measured and reconciled | `architecture/overview.md` and `architecture/count_audit_history.md` | pass | 87→88 and 48→49 |
| §13.1 — a multiline composer takes all four keys, fitting or not | `tui_composer_claims::a_multiline_composer_takes_the_vertical_keys`, `tui_composer_claims::k_is_claimed_when_the_composers_content_spans_more_than_one_row`, `tui_composer_motions::a_multiline_composer_takes_the_page_keys`, `tui_composer_motions::ctrl_d_pages_the_composer_when_its_content_spans_more_than_one_row` | pass | Both the multiline-fitting and the multiline-overflowing cases are asserted |
| §13.2 — a one-row composer leaves all four keys to the transcript | `tui_composer_claims::a_single_row_composer_leaves_the_vertical_keys_to_the_transcript`, `tui_composer_motions::a_single_row_composer_leaves_the_page_keys_to_the_transcript`, `tui_composer_motions::the_vertical_motions_do_not_apply_when_the_content_is_a_single_row` | pass | — |
| §13.3 — the page in a fitting composer moves the caret | `tui_composer_motions::a_multiline_composer_takes_the_page_keys` | pass | The caret is the assertion, not only the scroll |
| §13.5 — one rule in one wording, proven by the guard | the guard's passing run below | pass | Seven published locations carry the phrase |
| §13.9 — `verify.sh quick` passes and clippy is clean | the verification section below | pass | — |

## 3. Production implementation evidence

### The predicate

```rust
// src/tui/composer_claims.rs
pub fn is_multiline(self) -> bool {
    match self {
        Self::Unknown => false,
        Self::Measured { row_count, .. } => row_count > 1,
    }
}
```

It reads the fit the layout pass already cached, so it is **not** a second
measurement of the same fact — the stop condition in §14 is not reached.

`PromptWidget::is_multiline()` (`src/tui/components/prompt.rs:757`) is the
widget's thin delegate to it. It is the *same* predicate, not a second
answer: it holds no arithmetic of its own.

**Its two callers**, exactly as §6.1 requires:

1. **The claim gate** — `ClaimCondition::ContentSpansMultipleRows::holds`
   (`src/tui/composer_claims.rs:360`), which is what `j` and `k` are gated by.
2. **The app-level page routing** — the `PageUp` and `PageDown` arms of the
   key chain (`src/tui/app/mod.rs:4428` and `src/tui/app/mod.rs:4441`), which
   decide between `prompt.page_up()`/`page_down()` and the transcript's
   `scroll_page_up()`/`scroll_page_down()`.

### What happened to the old predicates

- **`PromptWidget::content_overflows()`** — **replaced** by
  `PromptWidget::is_multiline()`. It was the app-level routing's only overflow
  predicate; leaving it behind would have kept the old question alive under a
  second name, and the gate and the page branch would then have answered two
  different questions about one composer. Every call site was re-pointed:
  two in `src/tui/app/mod.rs`, the widget's own tests in
  `src/tui/components/prompt.rs`, and the render members in
  `tests/tui_render/tui_composer_claims.inc` and
  `tests/tui_render/tui_composer_motions.inc`.
- **`ComposerFit::overflows()`** — **removed**. After the claim condition was
  re-pointed, nothing called it. It was left removed rather than kept as a
  second answer to a question nobody asks any more.

### The condition

The variant is **`ClaimCondition::ContentSpansMultipleRows`**, and its label is
exactly:

```
composer content spans more than one row
```

`ClaimCondition::Always` still has no label (`None`).

### The behaviour before and after

The four keys are `j`, `k`, `Ctrl+u` and `Ctrl+d`. The three composer states
are the three the milestone names.

| Composer state | `j` / `k` — before | `j` / `k` — after | `Ctrl+u` / `Ctrl+d` — before | `Ctrl+u` / `Ctrl+d` — after |
|---|---|---|---|---|
| **One row** | the transcript's: `j`/`k` scroll the conversation | **unchanged** — the transcript's | the transcript's: `Ctrl+u`/`Ctrl+d` page the conversation | **unchanged** — the transcript's |
| **Several rows that fit the box** | the transcript's: `j`/`k` scroll the conversation | **the composer's**: `j`/`k` move the caret one **visual row** | the transcript's: `Ctrl+u`/`Ctrl+d` page the conversation | **the composer's**: the page moves the window **and the caret** |
| **Several rows that overflow the box** | the composer's: `j`/`k` move the caret one visual row | **unchanged** — the composer's | the composer's: the page moves the window and the caret | **unchanged** — the composer's |

The recovery is **automatic**: when the composer's content returns to one row,
the four keys belong to the transcript again, and no key is needed to make that
happen.

### The ADR amendment

The amendment is a **dated appended section**, in the shape the 2026-10-23
amendment established, placed after that amendment and before `## Supersession`.
The Decision text is **not** edited.

> ## Amendment: the vertical keys follow a multiline composer
>
> **Amended 2026-10-24.**
>
> The 2026-10-23 amendment above set the `j`/`k` condition against the box's
> **fit**. The condition is now the composer's **rendered row count**: the
> vertical keys belong to the composer while its content occupies more than one
> row, and they return to the transcript when it is down to one row again.

The amendment records, in full: the **change** (the effective condition on
Decision part 4's `j`/`k` rows, and the note that the Decision text itself is
not edited); what is **preserved** (the 2026-10-23 reason verbatim — a wrapped
single logical line that overflows its box stays reachable, because an
overflowing composer is necessarily multiline — and the visual-row nature of
the motion); what **changes** (the boundary moves in the other direction, and
the recovery is automatic); the **trade-off** (while the composer is
multiline, the transcript's `j`/`k` are unavailable, and the alternative would
leave the composer's own rows unreachable by the keys it publishes); the
app-level `Ctrl+u`/`Ctrl+d` routing following the **same predicate** and adding
**no** claim-table row; and a before/after table plus the "what the amendment
does not change" list.

### The one wording, in all seven published places

The string is:

```
composer content spans more than one row
```

| # | Location | How it appears |
|---|---|---|
| 1 | `src/tui/composer_claims.rs` — `ClaimCondition::label()` | `Some("composer content spans more than one row")` |
| 2 | `architecture/tui.md` — the claim table's `Condition` cell for `j` | `**composer content spans more than one row**` |
| 3 | `architecture/tui.md` — the claim table's `Condition` cell for `k` | `**composer content spans more than one row**` |
| 4 | `architecture/tui.md` — the `Ctrl+u` / `Ctrl+d` page paragraph | "**`Ctrl+u` and `Ctrl+d` page the composer when the composer content spans more than one row, and the transcript when it does not**, …" |
| 5 | `docs/tui.md` — the prose paragraph about `j` and `k` | "`j` and `k` only move the caret when the **composer content spans more than one row** — …" |
| 6 | `docs/tui.md` — the prose paragraph about `Ctrl+u` and `Ctrl+d` | "They page the **composer** when the composer content spans more than one row, and the **conversation** when it does not — …" |
| 7 | `docs/tui.md` — the `[NOR]`-mode keymap table's `j` / `k` row | "Move down / up a row of the box — only when the composer content spans more than one row" |

A seventh **published** location was added beyond the six the milestone names:
the `docs/tui.md` **word-motion** table's `j` / `k` row, which the guard parses
as a staleness-checked location and which becomes containment-checked once it
states a condition. It now reads "Move down / up a row of the box, keeping the
column — only when the composer content spans more than one row".

None of the seven contains `overflows the box`, `does not fit its box` or
`more than one line`.

### The guard

`scripts/check_tui_composer_keymap_drift.py` was written by a sibling agent
before this milestone and was **not rewritten** here; it was read, wired in, and
satisfied by the production change. Its rules:

- **D1** — the published keymaps' letter sets must equal the claim table's
  letter set (the implemented set is `a b d e i j k o p u U v w x`).
- **D2** — the published **conditional** set must equal the claim table's
  conditional set (today `{j, k}`).
- **D3** — the published claim table's `Condition` cell for each conditional
  letter must **contain** the implementation's condition label.
- **D4** — the published **prose** about the condition must contain the label,
  and none of the parsed locations may still contain the superseded phrasings
  `overflows the box`, `does not fit its box`, `more than one line`.
- **D5** — a table, a row or an input the rule needs cannot be found is a
  **violation**, not a clean tree.

Its matching rules normalize the text (lowercased, `*` and backticks stripped,
whitespace collapsed) and require the label as a **substring**. Containment is
strict, not semantic.

#### The armed run — before the change (exit 1, 9 violations)

```text
check_tui_composer_keymap_drift: rule D3: architecture/tui.md: the claim table's Condition cell for `j` (line 2159) says '**composer buffer has more than one line**', not the implementation's condition 'composer content overflows the box'
check_tui_composer_keymap_drift: rule D3: architecture/tui.md: the claim table's Condition cell for `k` (line 2160) says '**composer buffer has more than one line**', not the implementation's condition 'composer content overflows the box'
check_tui_composer_keymap_drift: rule D4: architecture/tui.md: the page paragraph (line 2738) states a condition that is not the implementation's 'composer content overflows the box': "**`Ctrl+u` and `Ctrl+d` page the composer when it overflows and the transcript when it does not**, in the same branch shape the workspace dashboard already uses. The page size is the transcript's own rule: the visible height minus two, with a floor of one. The page is **not mode-gated** — the same branch handles both modes."
check_tui_composer_keymap_drift: rule D4: docs/tui.md: the paragraph about `j` and `k` (line 379) states a condition that is not the implementation's 'composer content overflows the box': '`j` and `k` only move the caret when your prompt **does not fit its box** — when there is more of it than the box can show. When the prompt fits, they scroll the conversation instead, which is what they have always done.'
check_tui_composer_keymap_drift: rule D4: docs/tui.md: the paragraph about `Ctrl+u` and `Ctrl+d` (line 383) states a condition that is not the implementation's 'composer content overflows the box': '`Ctrl+u` and `Ctrl+d` **page**. They page the **composer** when the composer has more to show than its box can hold, and the **conversation** when it does not — the same two keys, and the same page size either way. The page is not restricted to `[NOR]`: it behaves the same in both modes.'
check_tui_composer_keymap_drift: rule D4: docs/tui.md: the `[NOR]`-mode keymap table's row for the conditional letters (line 399) states a condition that is not the implementation's 'composer content overflows the box': '`j` / `k` | Move down / up a row of the box — only when the prompt does not fit its box'
check_tui_composer_keymap_drift: rule D4: architecture/tui.md: the claim table's Condition cell for `j` (line 2159) still says 'more than one line', which the correction supersedes
check_tui_composer_keymap_drift: rule D4: architecture/tui.md: the claim table's Condition cell for `k` (line 2160) still says 'more than one line', which the correction supersedes
check_tui_composer_keymap_drift: rule D4: docs/tui.md: the `[NOR]`-mode keymap table's row for the conditional letters (line 399) still says 'does not fit its box', which the correction supersedes
check_tui_composer_keymap_drift: 9 violation(s)
```

#### The passing run — after the change (exit 0)

```text
check_tui_composer_keymap_drift: the published composer keymap and the implemented claim table agree
```

#### The self-test (exit 0, 10 cases)

```text
self-test: a clean tree finds nothing — the published keymap and the claim table agree
self-test: a letter the claim table has and the published table drops is rule D1 — the published claim table does not name `a`
self-test: a letter the published table has and the claim table does not is rule D1 — the published claim table names `z`
self-test: a published condition the implementation does not have is rule D2 — the published table marks `a` conditional
self-test: a conditional claim the published table calls unconditional is rule D2 — the published table marks `k` unconditional
self-test: a published condition in other words is rule D3 — the published cell does not carry the implementation's wording
self-test: published prose that states another rule is rule D4 — the paragraph does not carry the implementation's wording
self-test: a published surface that still says the superseded rule is rule D4 — the keymap table still carries the superseded phrasing
self-test: a table the guard needs but cannot find is rule D5 — the published claim table is not where the guard looks for it
self-test: a row the guard needs but cannot find is rule D5 — the word-motion table has no row for the conditional claim
self-test: 10 cases passed
```

#### Registration in both gates

`scripts/verify.sh`, in `run_guards()`, following the
`scripts/check_tui_files_keymap_drift.py` precedent — a `--self-test`
invocation and a plain invocation, both wrapped in `(cd "$REPO_ROOT" && …)`:

```bash
    echo "==> python3 scripts/check_tui_composer_keymap_drift.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_tui_composer_keymap_drift.py --self-test)
    echo "==> python3 scripts/check_tui_composer_keymap_drift.py"
    (cd "$REPO_ROOT" && python3 scripts/check_tui_composer_keymap_drift.py)
```

`.github/workflows/ci.yml`, in the same step style:

```yaml
      - name: Composer keymap drift self-test
        run: python3 scripts/check_tui_composer_keymap_drift.py --self-test

      - name: Composer keymap drift guard
        run: python3 scripts/check_tui_composer_keymap_drift.py
```

### The counts

`python3 scripts/measure_overview_counts.py` against this tree:

| Row | Measured before | Measured after | Published before | Published after |
|---|---|---|---|---|
| Guard scripts (all) | **88** | **88** | 87 | **88** |
| CI guard scripts | **48** | **49** | 48 | **49** |

The reconciliation, recorded in `architecture/count_audit_history.md` under
`## C003 — the composer's keymap drift guard (2026-10-24)`:

- The guard **script** was written by a sibling agent before this milestone and
  was already in the tree, so the *Guard scripts (all)* row had already moved
  to 88 while `architecture/overview.md` still published 87 — a drift the
  milestone's registration reconciled.
- The guard's **registration** in `.github/workflows/ci.yml` is what moved the
  *CI guard scripts* row from 48 to 49. The row counts the script once, not
  the two CI steps.
- No other row moved: no schema, no tool registration, no module, no storage
  layout, and **no** integration test file — the new tests extend the existing
  members `tests/tui_render/tui_composer_claims.inc` and
  `tests/tui_render/tui_composer_motions.inc`, which the *Integration test
  files* row already counted at 335.
- The *quick runs 14 guards CI does not* note is unchanged: the new guard is in
  **both** `quick` and CI. The set was re-derived rather than carried over, by
  differencing the guards `run_guards()` names against the guards
  `.github/workflows/ci.yml` names: **63** in `quick`, **49** in CI, and the
  same 14 quick-only guards as before.

### The comment budget

`python3 scripts/check_comment_budget.py` broke on this change — the milestone
adds documentation to four files that were at their recorded ceilings. The
budget's documented procedure is to re-measure, so
`python3 scripts/check_comment_budget.py --bank` was run and the diff is
confined to this milestone's four files and the three global values:

```diff
 [global]
-max_comment_lines = 122867
-max_comment_words = 1082066
+max_comment_lines = 122920
+max_comment_words = 1082625
-"src/tui/app/mod.rs" = { max_comment_lines = 2941, max_ratio = 0.1748 }
+"src/tui/app/mod.rs" = { max_comment_lines = 2942, max_ratio = 0.1749 }
-"src/tui/components/prompt.rs" = { max_comment_lines = 590, max_ratio = 0.4698 }
+"src/tui/components/prompt.rs" = { max_comment_lines = 596, max_ratio = 0.4716 }
-"src/tui/composer_claims.rs" = { max_comment_lines = 314, max_ratio = 0.6488 }
+"src/tui/composer_claims.rs" = { max_comment_lines = 332, max_ratio = 0.6472 }
-"tests/tui_render/tui_composer_claims.inc" = { max_comment_lines = 210, max_ratio = 0.3421 }
+"tests/tui_render/tui_composer_claims.inc" = { max_comment_lines = 225, max_ratio = 0.3549 }
-"tests/tui_render/tui_composer_motions.inc" = { max_comment_lines = 112, max_ratio = 0.3673 }
+"tests/tui_render/tui_composer_motions.inc" = { max_comment_lines = 125, max_ratio = 0.3754 }
```

## 4. Verification executed

### Commands run

```bash
cd /Users/davidbowman/projects/codegg/.worktrees/feat-auto-20261023-69770c99
python3 scripts/check_tui_composer_keymap_drift.py            # BEFORE the change: must fail (armed evidence)
cargo fmt --all
cargo test -p codegg --lib -- composer_claims
cargo test -p codegg --lib -- tui::components::prompt
cargo test --test tui_render -- tui_composer_claims tui_composer_motions modifier_chords
cargo test --test tui_render
python3 scripts/check_tui_composer_key_authority.py
python3 scripts/check_tui_composer_key_authority.py --self-test
python3 scripts/check_tui_composer_keymap_drift.py            # AFTER the change: must pass
python3 scripts/check_tui_composer_keymap_drift.py --self-test
python3 scripts/measure_overview_counts.py
python3 scripts/check_comment_budget.py
CARGO_BUILD_JOBS=3 cargo clippy -p codegg --all-targets --locked -- -D warnings
bash scripts/verify.sh quick
```

### Results

| Command | Result | Observed |
|---|---|---|
| `check_tui_composer_keymap_drift.py` (before) | **fail, as required** | 9 violations, exit 1 — the exact output is in §3 |
| `cargo fmt --all` | pass | no diff reported |
| `cargo test -p codegg --lib -- composer_claims` | pass | **16 passed**, 0 failed |
| `cargo test -p codegg --lib -- tui::components::prompt` | pass | **35 passed**, 0 failed |
| `cargo test --test tui_render -- tui_composer_claims tui_composer_motions modifier_chords` | pass | **67 passed**, 0 failed |
| `cargo test --test tui_render` (the whole family) | pass | **1093 passed**, 0 failed, 0 ignored, 0 filtered out |
| `check_tui_composer_key_authority.py` | pass | "Composer key-ownership guard passed — the claim table is closed, single-owner, leaks no key to an app-level action, and every caret motion is bound the same way in both maps" |
| `check_tui_composer_key_authority.py --self-test` | pass | "self-test passed: every violation class was detected and rejected" |
| `check_tui_composer_keymap_drift.py` (after) | pass | "the published composer keymap and the implemented claim table agree", exit 0 |
| `check_tui_composer_keymap_drift.py --self-test` | pass | 10 cases passed, exit 0 |
| `measure_overview_counts.py` | pass | Guard scripts (all) **88**, CI guard scripts **49**, Integration test files 335, Architecture docs 92 |
| `check_comment_budget.py` | pass, after re-banking | 11 violations before the re-bank; "comment budget guard passed" after |
| `cargo clippy -p codegg --all-targets --locked -- -D warnings` | pass | no warnings, exit 0 |
| `bash scripts/verify.sh quick` | pass | exit 0 — see below |

`scripts/verify.sh quick` ran the routine subset — rustfmt, the two
generated-asset checks, the guard scripts (now including the new
`check_tui_composer_keymap_drift.py` and its `--self-test`), and the workspace
`cargo check` — and exited **0**.

### Not run, and why

- `cargo nextest run --workspace`, `cargo test --workspace`, a bare
  `cargo test`, and any `--all-features` sweep were **not** run: the
  milestone's instructions forbid them, and the permitted shapes cover the
  change (the package's library tests, the one affected integration family,
  and package-scoped clippy over every target).
- `scripts/verify.sh full` was not run; its extra scope is the workspace test
  sweep and the feature-gated suite, which the instructions exclude.
- Hosted CI was not run: the milestone's two CI steps are the guard's
  `--self-test` and the guard itself, and both were run locally and pass.

## 5. Invariant review

| Invariant | Evidence | Result |
|---|---|---|
| **I1** — one fact, one predicate | `ComposerFit::is_multiline()` is the only implementation of the question; `PromptWidget::is_multiline()` holds no arithmetic of its own | pass |
| **I2** — the rendered row count is the measure | `row_count > 1`; `ComposerFit::overflows()` is removed and no overflow predicate survives | pass |
| **I3** — an unmeasured composer is not multiline | `ComposerFit::Unknown => false`; `composer_claims::an_unknown_fit_does_not_hold_the_condition`; `tui_composer_claims::the_claim_is_not_live_before_the_composer_has_been_laid_out` | pass |
| **I4** — the claim gate is consulted before the binding map | unchanged — the change is the *condition*, not the gate's position | pass |
| **I5** — the claim set is unchanged | the same fourteen letters, `a b d e i j k o p u U v w x`; `composer_claims::the_table_is_the_adrs_fixed_letter_set` passes | pass |
| **I6** — `j`/`k` are the composer's only while the condition holds | `tui_composer_claims::a_single_row_composer_leaves_the_vertical_keys_to_the_transcript` and `tui_composer_claims::a_multiline_composer_takes_the_vertical_keys` | pass |
| **I7** — the page moves the caret with the window | `tui_composer_motions::a_multiline_composer_takes_the_page_keys` asserts the **caret** moved; `prompt::the_page_motion_pages_the_window_and_the_caret_together` passes | pass |
| **I8** — the transcript's scroll is unchanged | the one-row case still pages the transcript: `tui_composer_motions::a_single_row_composer_leaves_the_page_keys_to_the_transcript` | pass |
| **I9** — no new key space | the `Ctrl+u`/`Ctrl+d` chords are app-level and add **no** claim-table row; `composer_claims::the_table_is_closed_and_single_owner` passes | pass |
| **I10** — the published rule is one wording | the guard's passing run proves D1–D5 | pass |

## 6. Failure and recovery review

- **No durable state, no schema, no protocol.** The milestone changes a
  predicate, a condition label, two published documents, one ADR section and
  two gate registrations. There is nothing to migrate and nothing to roll back
  beyond reverting the change.
- **The predicate's failure mode is deliberate.** An unmeasured composer
  (`ComposerFit::Unknown`) is **not** multiline, so the keys fall through to
  the transcript rather than being swallowed in a state the user cannot see.
  This is invariant I3 and it is unchanged by the milestone.
- **The recovery is automatic.** When the composer's content returns to one
  row, the four keys belong to the transcript again — no key, no command and no
  state is needed to make that happen, and no stale claim can survive it,
  because the condition is re-evaluated from the cached fit on every gate
  consultation.
- **The guard's failure mode is a failure.** A published surface that cannot
  be parsed is a **violation** (rule D5), not a clean tree, so a guard that
  cannot find the tables cannot pass by accident.

## 7. Migration and compatibility review

- **No schema migration.** No table, migration or storage layout changed.
- **No protocol change.** The claim table is internal to the TUI.
- **Backward compatibility**: the composer keymap is a user-facing behaviour, so
  a user who relied on `j`/`k` scrolling the transcript while their composer
  held several rows that fit will see a change — that **is** the milestone's
  intent, and it is the change ADR-0042's new dated amendment records.
  Nothing else in the product changes behaviour.
- **Rollback limitation**: reverting the change restores the old condition and
  makes the new guard fail again (its D3/D4 rules), so the guard and the
  implementation must be reverted together.

## 8. Security review

- No authorization, secret handling, path validation, privilege boundary,
  redaction or audit surface changed. The milestone touches key **routing**,
  not key **authority**: the claim table's key space, its single-owner rule and
  its leak rule are unchanged, and `check_tui_composer_key_authority.py`
  passes both on the tree and on its self-test.
- **Denial-of-service bounds**: none changed. The condition is a comparison of
  two integers the layout pass already computed.

## 9. Documentation and operations

| Document | Change |
|---|---|
| `architecture/tui.md` | the claim table's `j` and `k` condition cells; the paragraph naming `ClaimCondition::ContentSpansMultipleRows`; the paragraph about the measure and the exact-fit case; the selection section's conditional sentence; the `Ctrl+u`/`Ctrl+d` page paragraph |
| `docs/tui.md` | the word-motion table's `j`/`k` row; the `j`/`k` prose paragraph; the `Ctrl+u`/`Ctrl+d` prose paragraph; the `[NOR]`-mode keymap table's `j`/`k`, `b`, `e` and `w` rows; the two bullets that restated the old rule |
| `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` | the appended, dated `## Amendment: the vertical keys follow a multiline composer` section |
| `architecture/overview.md` | the verified-counts table's two guard rows |
| `architecture/count_audit_history.md` | the `## C003 — the composer's keymap drift guard (2026-10-24)` reconciliation |

**A leftover from C002 was finished here.** C002 changed the `v` row of
`docs/tui.md`'s `[NOR]`-mode table but, per its ownership, left the `e` and `b`
rows reading "Move to the last character of the word at the caret" / "Move to
the start of the previous word". Under C002's select mode those two verbs now
**select** the word they land on, so both rows now say so, and the `w` row now
says explicitly that `w` does **not** select.

**Static guards added**: `scripts/check_tui_composer_keymap_drift.py`, registered
in `scripts/verify.sh quick` and in `.github/workflows/ci.yml`. Its
`KNOWN_CONDITIONS` sibling, `scripts/check_tui_composer_key_authority.py`, was
updated to learn the new condition's name.

**Operator diagnostics**: none. There is no configuration, no runtime switch
and no log line for this behaviour.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | `AGENTS.md` publishes "62 guard scripts" for `quick` and "48 guard scripts" for CI; the measured values are now **63** and **49** | A stale count in a project instruction file; no guard reads it | Update `AGENTS.md`'s two counts. `AGENTS.md` is **outside this milestone's file ownership**, so it was not edited here — the drift is recorded in `architecture/count_audit_history.md` |
| low | The plan named `tests/tui_render/modifier_chords.inc` as the home for the two page-key members | The members live in `tests/tui_render/tui_composer_motions.inc` instead | None. `modifier_chords.inc` is a pure key-resolution family with no `App` harness; putting the page members there would have duplicated the whole harness, and `check_test_duplication.py` forbids asserting the same thing twice. The existing page members were re-pointed in place instead, and the plan's member names were adopted verbatim |

No critical, high or medium finding remains open.

## 11. Roadmap disposition

**Milestone closed and the next dependency may proceed.** Every acceptance
criterion in the plan's §13 is met with recorded evidence: the predicate and
its two callers are named, the four keys' behaviour is tabulated for the three
composer states, the ADR carries its dated amendment, the guard is registered
in both gates and was proven armed against the pre-correction tree, the counts
are re-measured and reconciled, and `verify.sh quick` and clippy both pass.

## 12. Registry updates

- `plans/registry.md` — **not** updated by this milestone. The registry is
  outside the milestone's file ownership; the C003 entry it needs is the
  parent task's to make.
- `plans/subsystems/tui-composer-helix-parity-post-closure-corrective-addendum.md`
  — **not** updated by this milestone; it is a plan document and outside the
  ownership.
- `plans/subsystems/tui-composer-text-layout-roadmap.md` — **not** touched.
